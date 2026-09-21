<script lang="ts">
  import { labelledMarks, scrollTopFor, stripY, yearAt, type YearMark } from '../lib/timeline';

  let {
    marks,
    total,
    viewport,
    scrollTop,
    onscrub,
  }: {
    marks: YearMark[];
    /** Height of the grid's canvas, which the strip is a scale model of. */
    total: number;
    /** Height of the grid's viewport, for the clamp at the end of the scroll. */
    viewport: number;
    scrollTop: number;
    onscrub: (scrollTop: number) => void;
  } = $props();

  /** Strip pixels between two printed years: a line of the label's type plus air. */
  const MIN_LABEL_GAP = 16;

  let el: HTMLDivElement;
  let strip = $state(0);
  let hoverY = $state<number | null>(null);
  let dragging = $state(false);

  const labels = $derived(labelledMarks(marks, total, strip, MIN_LABEL_GAP));
  const here = $derived(yearAt(marks, scrollTop));
  const hoverYear = $derived(hoverY === null || strip <= 0 ? null : yearAt(marks, (hoverY / strip) * total));

  function localY(e: PointerEvent): number {
    return e.clientY - el.getBoundingClientRect().top;
  }

  function scrub(e: PointerEvent) {
    onscrub(scrollTopFor(localY(e), strip, total, viewport));
  }

  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0) return;
    // Capture, so a drag that strays off the narrow strip keeps scrubbing.
    el.setPointerCapture(e.pointerId);
    dragging = true;
    hoverY = localY(e);
    scrub(e);
  }

  function onpointermove(e: PointerEvent) {
    hoverY = Math.min(Math.max(localY(e), 0), strip);
    if (dragging) scrub(e);
  }

  function release() {
    dragging = false;
  }
</script>

<!-- Not in the tab order: the grid already scrolls from the keyboard, and the sidebar's
     year groups are the keyboard's way to a year. The role is for what a pointer does. -->
<div
  class="timeline focus-container"
  bind:this={el}
  bind:clientHeight={strip}
  role="slider"
  tabindex="-1"
  aria-label="Timeline"
  aria-orientation="vertical"
  aria-valuenow={here ?? 0}
  aria-valuetext={here === null ? '' : String(here)}
  {onpointerdown}
  {onpointermove}
  onpointerup={release}
  onpointercancel={release}
  onpointerleave={() => !dragging && (hoverY = null)}
>
  {#each labels as mark (mark.top)}
    <span class="year" style:top="{stripY(mark.top, total, strip)}px">{mark.year}</span>
  {/each}
  <span class="here" style:top="{stripY(scrollTop, total, strip)}px"></span>
  {#if hoverY !== null && hoverYear !== null}
    <span class="bubble" style:top="{hoverY}px">{hoverYear}</span>
  {/if}
</div>

<style>
  .timeline {
    position: relative;
    flex: none;
    width: 44px;
    height: 100%;
    overflow: visible;
    cursor: pointer;
    user-select: none;
    touch-action: none;
    border-left: 1px solid var(--line);
  }
  .year {
    position: absolute;
    left: 0;
    right: 0;
    color: var(--text-dim);
    font-size: var(--t-1);
    line-height: 14px;
    text-align: center;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
  }
  .here {
    position: absolute;
    left: 4px;
    right: 4px;
    height: 2px;
    margin-top: -1px;
    background: var(--accent);
    border-radius: 1px;
    pointer-events: none;
  }
  .bubble {
    position: absolute;
    right: 100%;
    margin-right: 6px;
    transform: translateY(-50%);
    padding: 3px var(--s-2);
    background: var(--raised);
    border-radius: var(--r-2);
    box-shadow: 0 0 0 1px var(--line), var(--shadow-menu);
    font-size: var(--t-2);
    font-weight: 600;
    white-space: nowrap;
    pointer-events: none;
    z-index: 5;
  }
</style>
