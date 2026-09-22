import { MIN_ZOOM, clampPan, clampZoom } from './nav';

/** Two is the fewest that is a comparison; four is where a 2x2 stops being legible at any
 *  window size worth supporting. */
export const MIN_PANES = 2;
export const MAX_PANES = 4;

export function canCompare(count: number): boolean {
  return count >= MIN_PANES && count <= MAX_PANES;
}

/** What a pane needs to draw itself and to say what distinguishes it from its neighbours.
 *  `edit` only records *whether* the photo carries one - the thumbnail key already accounts
 *  for it, and the full-size URL needs a cache-busting parameter when it is set. */
export interface ComparePane {
  id: number;
  fileName: string;
  width: number;
  height: number;
  takenAt: number;
  thumbKey: string;
  edit: boolean;
  loaded: boolean;
}

export interface CompareDeps {
  load(id: number): Promise<ComparePane>;
  onclose(): void;
  onerror(e: unknown): void;
}

/** Two to four photos side by side, at one zoom over one point.
 *
 *  Zoom and pan are stored **once**, not per pane, and that is the feature rather than an
 *  implementation choice: two photos at 200% on the same eye is the comparison a person is
 *  trying to make, and two photos each at their own zoom is not.
 *
 *  Everything here is pure and synchronous apart from `open`, so vitest covers it under
 *  `environment: 'node'` where a `.svelte` file cannot be rendered. */
export function createCompare(deps: CompareDeps) {
  let panes = $state<ComparePane[]>([]);
  let focus = $state(0);
  let zoom = $state(MIN_ZOOM);
  let pan = $state({ x: 0, y: 0 });

  /** The pointer id of a pan in progress, or null. A gesture has three endings - pointerup
   *  finishes it, Escape abandons it, and a touchscreen sends pointercancel and nothing
   *  else - and all three call `endPan`. A missed teardown here would leave a live id that
   *  blocks every later drag, which is the failure this project has already shipped once in
   *  the grid's rubber band. */
  let panPointer: number | null = null;

  return {
    get panes() {
      return panes;
    },
    get focus() {
      return focus;
    },
    get zoom() {
      return zoom;
    },
    get pan() {
      return pan;
    },

    /** Loads every pane before showing any of them: half a comparison is worse than none,
     *  and the panes are laid out by how many there are. */
    async open(ids: number[]) {
      if (!canCompare(ids.length)) return;
      try {
        const loaded = await Promise.all(ids.map((id) => deps.load(id)));
        panes = loaded;
        focus = 0;
        zoom = MIN_ZOOM;
        pan = { x: 0, y: 0 };
        panPointer = null;
      } catch (e) {
        // Reset zoom/pan/focus too, not just panes: `panes: []` is meant to mean "closed",
        // the same state `close()` leaves - not "closed, but still carrying whatever zoom
        // and pan a previous, already-closed comparison left behind" for a caller that reads
        // them before the next open.
        panes = [];
        focus = 0;
        zoom = MIN_ZOOM;
        pan = { x: 0, y: 0 };
        panPointer = null;
        deps.onerror(e);
      }
    },

    close() {
      panes = [];
      zoom = MIN_ZOOM;
      pan = { x: 0, y: 0 };
      panPointer = null;
      deps.onclose();
    },

    /** Zooms by `factor` about a point `originX`/`originY` from the pane's centre.
     *
     *  The pan correction is what keeps that point still: scaling by `k` moves a point at
     *  offset `d` to `k*d`, so the pan must take back `d * (k - 1)`. Without it the photo
     *  appears to slide out from under the pointer, which is exactly the feeling that makes
     *  a shared zoom useless for comparing. */
    zoomAt(factor: number, originX: number, originY: number, width: number, height: number) {
      const next = clampZoom(zoom * factor);
      const k = next / zoom;
      const x = pan.x - originX * (k - 1);
      const y = pan.y - originY * (k - 1);
      zoom = next;
      pan = clampPan(x, y, next, width, height);
    },

    panBy(dx: number, dy: number, width: number, height: number) {
      pan = clampPan(pan.x + dx, pan.y + dy, zoom, width, height);
    },

    focusPane(i: number) {
      if (i >= 0 && i < panes.length) focus = i;
    },

    nextPane() {
      if (panes.length > 0) focus = (focus + 1) % panes.length;
    },

    /** Whether pane `i` should ask for the full-size render rather than the preview.
     *
     *  At most one pane ever does. Full-size renders are serialised behind `RENDERING` in
     *  `protocol.rs`, so letting every pane upgrade would queue four 24 MP decodes and show
     *  nothing until the last finished; and below 100% the preview's pixels are all that can
     *  be seen anyway. */
    needsFullImage(i: number): boolean {
      return i === focus && zoom > MIN_ZOOM;
    },

    get panning() {
      return panPointer !== null;
    },

    beginPan(pointerId: number) {
      panPointer = pointerId;
    },

    endPan() {
      panPointer = null;
    },
  };
}

export type Compare = ReturnType<typeof createCompare>;
