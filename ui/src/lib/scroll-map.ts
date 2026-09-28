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
 *  too big for any wheel, goes through the map. The thumb drifts while scrolling and is put
 *  back where it belongs when scrolling stops (`settle`).
 *
 *  The map is proportional in the middle and 1:1 within `endZone()` of either end. A purely
 *  proportional one left a settled grid `v / ratio` px of DOM for the `v` px of layout above
 *  it: every wheel step near an end hit the DOM's edge and re-anchored, each time a ratio
 *  closer, until the DOM sat at 0 with the library a pixel or two short of its top and no
 *  scroll event left to fire. With the ends 1:1 the DOM's edge and the library's arrive
 *  together.
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
  /** The DOM position a write from code left, whose scroll event is still to come. */
  let expected: number | null = null;

  const isMapped = () => total > domMax;
  const domHeight = () => Math.min(total, domMax);
  const maxVirtual = () => Math.max(0, total - viewport);
  const maxDom = () => Math.max(0, domHeight() - viewport);
  const clampVirtual = (v: number) => Math.min(maxVirtual(), Math.max(0, v));

  /** How far from each end the map is 1:1: ten viewports, but never more than a quarter of
   *  the DOM range each, so the middle keeps half of it. */
  const endZone = () => Math.min(10 * viewport, maxDom() / 4);

  /** The virtual position a DOM position stands for: 1:1 within `endZone()` of each end,
   *  proportional between; exact at both ends so a thumb dragged to the bottom lands on the
   *  last row. The inverse of `toDom`. */
  function fromDom(d: number): number {
    const md = maxDom();
    const mv = maxVirtual();
    if (md === 0 || d <= 0) return 0;
    if (d >= md) return mv;
    const k = endZone();
    if (d <= k) return d;
    if (d >= md - k) return mv - (md - d);
    // Here k < d < md - k, so the middle span is not empty.
    return k + ((d - k) / (md - 2 * k)) * (mv - 2 * k);
  }

  /** The DOM position that stands for `v`; see `fromDom`. */
  function toDom(v: number): number {
    const md = maxDom();
    const mv = maxVirtual();
    if (mv === 0 || md === 0) return 0;
    const k = endZone();
    if (v <= k) return Math.min(v, md);
    if (v >= mv - k) return Math.max(0, md - (mv - v));
    // Here k < v < mv - k, so the middle span is not empty.
    return k + ((v - k) / (mv - 2 * k)) * (md - 2 * k);
  }

  /** Whether DOM position `d` and virtual position `v` are at the same edge, or neither
   *  at one. When they are not, the next step towards that edge goes nowhere: the DOM has
   *  walked into a wall the library has not reached (or the reverse). */
  function edgesAgree(d: number, v: number): boolean {
    return d < 1 === v <= 0 && d > maxDom() - 1 === v >= maxVirtual();
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

    /** The layout's height, the viewport's, and the cap - on every rebuild, resize, tile
     *  size change and new display scale. Called after the canvas has its new height: a
     *  write into a canvas not yet grown is clamped away. */
    resize(nextTotal: number, nextViewport: number, nextDomMax: number): number | null {
      const wasMapped = isMapped();
      const wasHeight = domHeight();
      total = nextTotal;
      viewport = nextViewport;
      domMax = nextDomMax;
      if (!wasMapped && !isMapped()) return null;
      if (!isMapped()) {
        // The canvas holds the whole layout again: the place to be is `virtual` itself.
        virtual = clampVirtual(virtual);
        pending = null;
        return virtual;
      }
      const clamped = clampVirtual(virtual);
      // Staying past the cap at the same height is the common case - a scan adding photos
      // during a flick - and writes nothing, so the flick carries on. Unless the grid was
      // at the end and the library grew under it: the DOM is still at its end, a wheel
      // step down fires no scroll event, and nothing else would move it.
      if (
        wasMapped &&
        domHeight() === wasHeight &&
        clamped === virtual &&
        lastDomTop >= 0 &&
        lastDomTop <= maxDom() &&
        edgesAgree(lastDomTop, virtual)
      ) {
        return null;
      }
      virtual = clamped;
      return target(virtual);
    },

    /** The DOM position to write for virtual position `v`. Under the cap `v` itself,
     *  unclamped: the browser clamps it, as it always has. */
    setVirtual(v: number): number {
      if (!isMapped()) {
        pending = null;
        return v;
      }
      return target(v);
    },

    /** The virtual position for a DOM position read now, ahead of its scroll event. */
    virtualAt(domTop: number): number {
      return isMapped() ? clampVirtual(domTop + shift) : domTop;
    },

    /** What the browser took from a write the caller just made, read back from the
     *  viewport: it may have rounded or clamped the value written. */
    wrote(domTop: number): void {
      lastDomTop = domTop;
      if (!isMapped()) {
        virtual = domTop;
        shift = 0;
        pending = null;
        expected = null;
        return;
      }
      if (pending !== null) virtual = pending;
      pending = null;
      shift = virtual - domTop;
      expected = domTop;
    },

    /** Scrolling has gone still: the DOM position that puts the thumb back where the virtual
     *  position is, or null when it is already there. Nothing on screen moves - `virtual`
     *  is unchanged and `shift` absorbs the difference. */
    settle(): number | null {
      expected = null;
      if (!isMapped()) return null;
      return Math.abs(toDom(virtual) - lastDomTop) < 1 ? null : target(virtual);
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
      // An elastic bounce reports a position past an end and then the end again: read as
      // it came, the step back in would move the library by the bounce's depth.
      domTop = Math.min(maxDom(), Math.max(0, domTop));
      // The event photon's own write caused: already applied by `wrote`. Within a pixel, not
      // equal - a scaled display reads a fractional position back and reports another.
      if (expected !== null && Math.abs(domTop - expected) < 1) {
        expected = null;
        lastDomTop = domTop;
        return null;
      }
      expected = null;
      const moved = domTop - lastDomTop;
      lastDomTop = domTop;
      if (onScrollbar || Math.abs(moved) > JUMP_VIEWPORTS * viewport) {
        // Ends map to ends exactly, so a mapped step never lands on one range's edge
        // without the other's.
        virtual = fromDom(domTop);
        shift = virtual - domTop;
        return null;
      }
      virtual = clampVirtual(virtual + moved);
      shift = virtual - domTop;
      // Re-anchored at once rather than on settle when the two ranges' edges disagree - it
      // can cut a flick short, but only after millions of px without a pause.
      return edgesAgree(domTop, virtual) ? null : target(virtual);
    },
  };
}

export type ScrollMap = ReturnType<typeof createScrollMap>;
