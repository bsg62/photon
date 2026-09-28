/** Keeping every photo reachable when the grid is taller than the browser will lay out.
 *
 *  Engines cap a box at (2^31 - 1) / 64 px - lower in CSS px on a scaled Windows display,
 *  where Chromium applies it in device pixels - and a canvas past it stops growing: every
 *  row beyond is laid out on the cap and `scrollTop` stops there. So the grid keeps two
 *  positions. *Virtual* is the layout's own (`buildRows`), what every consumer of the
 *  grid's `scrollTop` state reads. The *DOM* position is the viewport's `scrollTop` over a
 *  canvas held at `domHeight`. They differ by `shift`, and the mounted rows are drawn at
 *  `row.top - shift`.
 *
 *  Under the cap the two are the same and `shift` is 0 - the map is the identity, and a
 *  library that fits behaves exactly as it did before the map existed.
 *
 *  Past it, a DOM range shorter than the layout cannot be both 1:1 for the wheel and
 *  proportional for the thumb. The wheel wins: a small step moves the virtual position by
 *  the same amount (`shift` unchanged), and only a held press on the scrollbar, or a jump
 *  too big for any wheel, maps proportionally. The thumb drifts while scrolling and is put
 *  back where it belongs when scrolling stops (`settle`).
 *
 *  Every method that returns `number | null` returns a DOM `scrollTop` for the caller to
 *  write - and then report with `wrote` - or null for none. */

/** The share of the measured cap the canvas may use: room for the rest of the viewport's
 *  content (the Copies notice below the canvas) and for an engine rounding differently. */
export const CAP_MARGIN = 0.9;
/** Used until a trustworthy reading exists: under the cap of every engine measured, at
 *  every display scale up to 4x (Chromium at 2x: 16,777,214). */
export const CAP_FALLBACK = 8_000_000;
/** A probe reading below this came from a window not yet laid out, not from an engine. */
export const CAP_MIN_READING = 1_000_000;
/** How tall the probe asks to be: far past every engine's cap. */
export const PROBE_HEIGHT = 1e9;
/** A single scroll event moving the DOM more than this many viewports is not a wheel: a
 *  track click, or a thumb drag on an overlay scrollbar no press could be seen on. */
export const JUMP_VIEWPORTS = 2;

/** The canvas height to allow, from the height a `PROBE_HEIGHT` box was given; null when
 *  the reading cannot be trusted (see `CAP_MIN_READING`). */
export function capFrom(measured: number): number | null {
  if (!(measured >= CAP_MIN_READING)) return null;
  return Math.floor(measured * CAP_MARGIN);
}

export function createScrollMap() {
  let total = 0;
  let viewport = 0;
  let domMax = Number.POSITIVE_INFINITY;
  let virtual = 0;
  let shift = 0;
  let lastDomTop = 0;
  let onScrollbar = false;
  /** The virtual position a write the caller is about to make will take the grid to. */
  let pending: number | null = null;

  const isMapped = () => total > domMax;
  const domHeight = () => Math.min(total, domMax);
  const maxVirtual = () => Math.max(0, total - viewport);
  const maxDom = () => Math.max(0, domHeight() - viewport);
  const clampVirtual = (v: number) => Math.min(maxVirtual(), Math.max(0, v));

  /** The virtual position a DOM position stands for, proportionally; exact at both ends so
   *  a thumb dragged to the bottom lands on the last row. */
  function fromDom(d: number): number {
    const md = maxDom();
    if (md === 0 || d <= 0) return 0;
    if (d >= md) return maxVirtual();
    return (d / md) * maxVirtual();
  }

  /** The DOM position that stands for `v`, proportionally. */
  function toDom(v: number): number {
    const mv = maxVirtual();
    return mv === 0 ? 0 : (v / mv) * maxDom();
  }

  /** Aims the grid at virtual `v`: the DOM position to write for it, past the cap. */
  function target(v: number): number {
    pending = clampVirtual(v);
    return toDom(pending);
  }

  return {
    get virtual() {
      return virtual;
    },
    get shift() {
      return shift;
    },
    get domHeight() {
      return domHeight();
    },
    get mapped() {
      return isMapped();
    },

    /** A press on the viewport: `scrollbar` when it landed on the scrollbar itself. */
    press(scrollbar: boolean): void {
      onScrollbar = scrollbar;
    },
    /** The press is over - or was lost, which the grid infers from a pointer moving with no
     *  button held. */
    release(): void {
      onScrollbar = false;
    },

    /** The layout's height, the viewport's, and the cap. (Completed in Task 4.) */
    resize(nextTotal: number, nextViewport: number, nextDomMax: number): number | null {
      const wasMapped = isMapped();
      total = nextTotal;
      viewport = nextViewport;
      domMax = nextDomMax;
      if (!isMapped()) return null;
      if (!wasMapped) return target(virtual);
      virtual = clampVirtual(virtual);
      shift = virtual - lastDomTop;
      return null;
    },

    /** What the browser took from a write the caller just made, read back from the
     *  viewport: it may have rounded or clamped the value written. */
    wrote(domTop: number): void {
      lastDomTop = domTop;
      if (!isMapped()) {
        virtual = domTop;
        shift = 0;
        pending = null;
        return;
      }
      if (pending !== null) virtual = pending;
      pending = null;
      shift = virtual - domTop;
    },

    /** A scroll event, with the DOM position it left. */
    onScroll(domTop: number): number | null {
      if (!isMapped()) {
        // Not clamped to the canvas: the Copies notice below it lets the viewport scroll a
        // little past the layout's end, and the browser's position is the truth there.
        virtual = domTop;
        shift = 0;
        lastDomTop = domTop;
        return null;
      }
      const moved = domTop - lastDomTop;
      lastDomTop = domTop;
      if (onScrollbar || Math.abs(moved) > JUMP_VIEWPORTS * viewport) {
        virtual = fromDom(domTop);
      } else {
        virtual = clampVirtual(virtual + moved);
      }
      shift = virtual - domTop;
      return null;
    },
  };
}

export type ScrollMap = ReturnType<typeof createScrollMap>;
