<script lang="ts">
  import { onMount } from 'svelte';
  import { api, errorMessage, mediaUrl, type ViewerItem } from '../lib/api';
  import {
    createCompare,
    differingFacts,
    MAX_PANES,
    type ComparePane,
  } from '../lib/compare.svelte';
  import { library } from '../lib/library.svelte';
  import { MIN_ZOOM } from '../lib/nav';
  import { createStarToggle, type StarToggle } from '../lib/star-toggle.svelte';
  import Icon from './Icon.svelte';

  interface Props {
    ids: number[];
    onclose: () => void;
    /** Opens a photo in the viewer, if the host offers one. Optional so the overlay is
     *  usable without it: Enter then only closes, which is what it would do anyway. */
    onopen?: (id: number) => void;
  }
  let { ids, onclose, onopen }: Props = $props();

  let root: HTMLDivElement | undefined = $state();
  let error = $state<string | null>(null);

  /** One star per pane, keyed by item id, each bound to what the load reported. The viewer's
   *  toggle is reused rather than calling `setStar` directly, because it is what makes the
   *  flip optimistic and the revert-on-failure safe; four of them because the focus moves
   *  between four photos and a single toggle would need rebinding on every move.
   *
   *  A plain Map, not `$state`, and that is safe only because of how this component lives:
   *  `open(ids)` runs once in `onMount` and awaits every load, so every entry exists before
   *  first paint, and App mounts `<Compare>` under `{#if compareIds !== null}` so each
   *  comparison gets its own instance. The badge then renders from each toggle's own rune.
   *  A pane added after first paint - a `<Compare>` handed new `ids` without remounting -
   *  would silently show no badge and keep the previous comparison's entries. That is the
   *  assumption to check before making `ids` live. */
  const stars = new Map<number, StarToggle>();

  const compare = createCompare({
    load: async (id) => {
      const it: ViewerItem = await api.viewerItem(id);
      const toggle = createStarToggle(api.setStar);
      toggle.bind(it.id, it.starred);
      stars.set(it.id, toggle);
      return {
        id: it.id,
        fileName: it.fileName,
        width: it.width,
        height: it.height,
        takenAt: it.takenAt,
        thumbKey: it.thumbKey,
        // Carried because `viewer_item` leaves an unedited photo's dimensions stored-side-up;
        // `differingFacts` swaps them, the way the viewer's caption does.
        orientation: it.orientation,
        // Only whether there is one: `thumbKey` already accounts for the edit, and the
        // full-size URL needs the key as a cache-buster when the photo carries one.
        edit: it.edit !== null,
      };
    },
    // Through a closure, not by reference: a prop read at construction captures only its
    // initial value.
    onclose: () => onclose(),
    onerror: (e) => (error = errorMessage(e)),
  });

  /** Ids whose full render failed. The overlay is dropped rather than left on top: it sits
   *  at `inset: 0` over a preview that is fine, so a broken-image affordance there would
   *  hide a picture the person can actually use. Per comparison, which is per mount. */
  let fullBroken = $state<number[]>([]);

  /** Ids whose *preview* failed. `protocol.rs` answers 503 while a thumbnail is still being
   *  rendered and 404 when the row is gone and nothing is cached under the key, so a photo
   *  deleted under an open comparison before its preview was built would otherwise leave a
   *  broken-image glyph with nothing to explain it. (One whose preview is cached goes on
   *  showing it: the key alone answers that, the way a preview already loaded stays up.)
   *  Unlike `fullBroken` the pane has nothing behind it to fall back to, so it says so in
   *  words. */
  let previewBroken = $state<number[]>([]);

  /** Dimensions and capture time, blank where every pane agrees. The rule is in
   *  `compare.svelte.ts` so that it can be tested; here it is a lookup. */
  const facts = $derived(differingFacts(compare.panes));

  onMount(() => {
    // The keys live on this element, so it has to hold focus from the first frame.
    root?.focus();
    void compare.open(ids);
  });

  /** How much one wheel notch zooms. Smaller than the viewer's slider steps because the
   *  wheel is the only zoom here, and overshooting the detail being compared costs the
   *  whole gesture. */
  const WHEEL_FACTOR = 1.15;

  /** The pointer capture belonging to a live pan, remembered so that *every* ending can
   *  release it - including Escape, which never reaches a pointer handler. A capture left
   *  behind blocks every later drag on that element, which is the failure this project
   *  shipped once in the grid's rubber band. */
  let captured: { el: HTMLElement; id: number } | null = null;
  let dragFrom = { x: 0, y: 0 };

  function centreOf(el: HTMLElement) {
    const r = el.getBoundingClientRect();
    return { r, cx: r.left + r.width / 2, cy: r.top + r.height / 2 };
  }

  function onwheel(e: WheelEvent & { currentTarget: HTMLElement }) {
    // Unprevented the webview scrolls the overlay itself, and the zoom lands on a photo
    // that has already moved under the pointer.
    e.preventDefault();
    const { r, cx, cy } = centreOf(e.currentTarget);
    const factor = e.deltaY < 0 ? WHEEL_FACTOR : 1 / WHEEL_FACTOR;
    compare.zoomAt(factor, e.clientX - cx, e.clientY - cy, r.width, r.height);
  }

  function onpointerdown(e: PointerEvent & { currentTarget: HTMLElement }, i: number) {
    if (e.button !== 0) return;
    // Pressing a pane is how it is chosen with the mouse; the choice decides which one
    // upgrades to the full render and which one S and Enter act on.
    compare.focusPane(i);
    // Nothing to pan at fit-to-pane, and starting a gesture there would only leave a
    // capture to tidy up.
    if (compare.zoom === MIN_ZOOM) return;
    dragFrom = { x: e.clientX, y: e.clientY };
    compare.beginPan(e.pointerId);
    e.currentTarget.setPointerCapture(e.pointerId);
    captured = { el: e.currentTarget, id: e.pointerId };
  }

  /** The pan is one offset in pane pixels, shared by every pane - the same thing
   *  `Viewer.svelte` does with its own rect. Each photo is `object-fit: contain`ed into its
   *  pane, so where the panes hold different aspect ratios the letterboxing differs and
   *  +100px is a different fraction of a landscape than of a portrait. For the burst of
   *  same-camera frames compare is for, the aspects match and it lands exactly. */
  function onpointermove(e: PointerEvent & { currentTarget: HTMLElement }) {
    if (!compare.panning) return;
    const r = e.currentTarget.getBoundingClientRect();
    compare.panBy(e.clientX - dragFrom.x, e.clientY - dragFrom.y, r.width, r.height);
    dragFrom = { x: e.clientX, y: e.clientY };
  }

  /** The one teardown all endings reach: pointerup, pointercancel (what a touchscreen sends
   *  when it claims the gesture, and nothing else follows) and Escape. Idempotent, so an
   *  Escape followed by the real pointerup costs nothing. */
  function endPan() {
    compare.endPan();
    if (captured && captured.el.hasPointerCapture(captured.id)) {
      captured.el.releasePointerCapture(captured.id);
    }
    captured = null;
  }

  function toggleStar(pane: ComparePane) {
    stars.get(pane.id)?.toggle().catch(library.reportError);
  }

  function onkeydown(e: KeyboardEvent) {
    // A live pan owns the keyboard, the way the grid's rubber band does: Escape abandons
    // the gesture and nothing else, so the comparison survives an abandoned drag.
    if (compare.panning) {
      if (e.key === 'Escape') {
        e.preventDefault();
        endPan();
      }
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      compare.close();
      return;
    }
    if (e.key === 'Tab') {
      // The panes are the only things to move between, so Tab moves between them rather
      // than walking out of the overlay - and Shift+Tab goes back, because a backwards Tab
      // that moves forwards is wrong however few panes there are.
      e.preventDefault();
      if (e.shiftKey) compare.prevPane();
      else compare.nextPane();
      return;
    }
    // Everything below is an unmodified key, as Viewer.svelte's letter keys are: a modifier
    // means the chord belongs to the webview or the OS, and Cmd+S is a reflex.
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    // Against `MAX_PANES` rather than a literal '4', so the digit keys cannot outlive a
    // change to how many panes a comparison holds.
    if (e.key >= '1' && e.key <= String(MAX_PANES)) {
      e.preventDefault();
      compare.focusPane(Number(e.key) - 1);
      return;
    }
    const pane = compare.panes[compare.focus];
    if (!pane) return;
    if (e.key.toLowerCase() === 's') {
      e.preventDefault();
      toggleStar(pane);
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      onopen?.(pane.id);
      compare.close();
    }
  }

  function previewSrc(p: ComparePane): string {
    return mediaUrl(`thumb/${p.id}/preview/${p.thumbKey}`);
  }

  /** The full render, for the one pane that needs it. The key is a cache-buster for an
   *  edited photo: `/image/<id>` is the same URL before and after a turn. */
  function fullSrc(p: ComparePane): string {
    return mediaUrl(`image/${p.id}`) + (p.edit ? `?k=${p.thumbKey}` : '');
  }
</script>

<!-- Dark in both themes, like the viewer: a photo is judged against the same ground
     whatever the app's theme. tokens.css's theme blocks match any element, so this subtree
     resolves the dark tokens rather than carrying a colour of its own. -->
<div
  class="compare focus-container"
  data-theme="dark"
  role="dialog"
  aria-modal="true"
  aria-label="Compare photos"
  tabindex="-1"
  bind:this={root}
  {onkeydown}
>
  <!-- A person who opened compare from the tile menu has been given no reason to know that
       Escape exists; the viewer answers this with the same button. It calls the factory's
       `close`, which is the one exit Escape takes too. -->
  <button class="close" onclick={() => compare.close()} aria-label="Close compare">
    <Icon name="x" size={16} />
  </button>
  {#if error}
    <p class="error">{error}</p>
  {:else}
    <div class="panes">
      {#each compare.panes as p, i (p.id)}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="pane"
          class:focused={i === compare.focus}
          {onwheel}
          onpointerdown={(e) => onpointerdown(e, i)}
          {onpointermove}
          onpointerup={endPan}
          onpointercancel={endPan}
        >
          <!-- The preview stays under the full render, so the pane never goes blank while
               the backend renders: full-size renders are serialised, so the gap is real. -->
          {#if previewBroken.includes(p.id)}
            <p class="gone">Couldn’t load this photo</p>
          {:else}
            <img
              class="shot"
              src={previewSrc(p)}
              alt={p.fileName}
              draggable="false"
              style="transform: translate({compare.pan.x}px, {compare.pan.y}px) scale({compare.zoom})"
              onerror={() => (previewBroken = [...previewBroken, p.id])}
            />
          {/if}
          {#if compare.needsFullImage(i) && !fullBroken.includes(p.id)}
            <img
              class="shot"
              src={fullSrc(p)}
              alt=""
              draggable="false"
              style="transform: translate({compare.pan.x}px, {compare.pan.y}px) scale({compare.zoom})"
              onerror={() => (fullBroken = [...fullBroken, p.id])}
            />
          {/if}
          <p class="label">
            <!-- Not `aria-hidden`: this digit is the only thing that says which number key
                 focuses which pane, so hiding it hides the affordance itself. `role="img"`
                 with a label for the same reason the star badge beside it has one. -->
            <span class="index" role="img" aria-label="Pane {i + 1}">{i + 1}</span>
            {#if stars.get(p.id)?.starred}
              <!-- `role="img"`, because an aria-label on a bare span is not reliably
                   exposed; the icon inside is decorative. -->
              <span class="star" role="img" aria-label="Starred"><Icon name="star" size={13} filled={true} /></span>
            {/if}
            <span class="name">{p.fileName}</span>
            {#if facts[i]?.size}<span class="fact">{facts[i].size}</span>{/if}
            {#if facts[i]?.taken}<span class="fact">{facts[i].taken}</span>{/if}
          </p>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .compare {
    position: fixed;
    inset: 0;
    z-index: 20;
    display: grid;
    grid-template-rows: 1fr;
    /* The ground is the dark theme's surface, from the subtree's own data-theme. */
    background: var(--surface);
    color: var(--text);
    overflow: hidden;
  }
  /* As the viewer's, top right. It sits over a pane rather than over empty chrome, so it
     takes the same scrim every plate on a photo takes - a bare glyph over a bright photo
     disappears. */
  .close {
    position: absolute;
    top: var(--s-3);
    right: var(--s-3);
    z-index: 1;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: var(--scrim);
    color: var(--text-dim);
    cursor: pointer;
  }
  .close:hover {
    color: var(--text);
  }
  .error {
    place-self: center;
    color: var(--danger);
  }
  .panes {
    display: grid;
    /* Two columns always: two panes fall side by side in one row, three and four make a
       2x2. Three leaves the fourth cell empty rather than stretching one pane over it, so
       every photo is shown at the same size - which is the comparison. Two columns and
       auto rows is a 2x2 only because MAX_PANES is 4; raising it needs this rule revisited,
       since a fifth pane would silently start a third row of half-height photos. */
    grid-template-columns: 1fr 1fr;
    grid-auto-rows: 1fr;
    gap: var(--s-1);
    padding: var(--s-1);
    min-height: 0;
  }
  .pane {
    position: relative;
    overflow: hidden;
    min-width: 0;
    min-height: 0;
    background: var(--chrome);
    border-radius: var(--r-2);
    /* The pointer owns every gesture: without this a touchscreen pan scrolls the overlay
       and sends pointercancel instead of the moves the pan is made of. */
    touch-action: none;
  }
  /* Inside the pane's own box, not an outline around it: the panes are packed against each
     other, and a ring outside one is clipped by its neighbour. */
  .focused::after {
    content: '';
    position: absolute;
    inset: 0;
    border: 2px solid var(--accent);
    border-radius: var(--r-2);
    pointer-events: none;
  }
  .shot {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
    transform-origin: center;
    user-select: none;
  }
  /* No photo behind it, so it sits where the photo would have been rather than over one. */
  .gone {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    margin: 0;
    padding: var(--s-3);
    text-align: center;
    font-size: var(--t-2);
    color: var(--text-dim);
  }
  .label {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    margin: 0;
    display: flex;
    align-items: center;
    gap: var(--s-2);
    padding: var(--s-1) var(--s-2);
    font-size: var(--t-2);
    /* It lies over the photo, so it takes the scrim that every plate on a photo takes. */
    background: var(--scrim);
    color: var(--text);
  }
  .index {
    flex: none;
    min-width: 1.4em;
    text-align: center;
    border-radius: var(--r-1);
    background: var(--field);
    color: var(--text-dim);
  }
  .star {
    flex: none;
    display: flex;
    color: var(--star);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fact {
    flex: none;
    color: var(--text-dim);
  }
</style>
