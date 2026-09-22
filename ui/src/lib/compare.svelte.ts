import { formatTaken } from './caption';
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

/** What a pane says about itself beyond its file name. Empty where every pane agrees.
 *
 *  `'×'` lives here rather than in the component for the same reason `formatCaption`'s
 *  does: `no-literals.test.ts` reads every `.svelte` file and fails on that glyph, because a
 *  glyph in markup is how icons used to be drawn. */
export interface PaneFacts {
  /** `4000 × 3000`. */
  size: string;
  /** The capture time, formatted as the viewer's caption formats it. */
  taken: string;
}

/** What distinguishes each pane from its neighbours.
 *
 *  Only a fact the panes disagree about is reported. Four identical `4000 × 3000` labels
 *  are noise a person has to read past; the point of comparing is the fact that decides
 *  between them, so a fact every pane shares is left off every pane. A fact that differs is
 *  shown on *every* pane, not only the odd one out - a lone number beside three blanks says
 *  nothing about the blanks.
 *
 *  `width`/`height` are the picture as shown: `viewer_item` reports an edited photo's
 *  dimensions after its turns and crop, so there is no orientation swap to do here.
 *
 *  Pure, and the one rule in this overlay that a test can hold: the rest of Compare.svelte is
 *  markup that vitest's node environment cannot render. */
export function differingFacts(panes: ComparePane[], locale?: string): PaneFacts[] {
  const differs = <T>(of: (p: ComparePane) => T) => panes.some((p) => of(p) !== of(panes[0]));
  const sizes = differs((p) => `${p.width}x${p.height}`);
  const takens = differs((p) => p.takenAt);
  return panes.map((p) => ({
    size: sizes ? `${p.width} × ${p.height}` : '',
    taken: takens ? formatTaken(p.takenAt, locale) : '',
  }));
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

  /** The one place all five pieces of state return to their closed values, together.
   *
   *  This module has had the same bug three times at three different sites - `open`'s
   *  failure branch resetting only `panes` and leaving `focus`/`zoom`/`pan` stale, `close`
   *  resetting everything except `focus`, and `open`'s success branch never touching
   *  `panPointer` - because each site re-listed the fields by hand and each one drifted on
   *  its own. Every exit from an open comparison (`close`, and both branches of `open`) calls
   *  this instead of assigning fields itself. A sixth piece of state belongs here too, or it
   *  is the fourth instance of the same bug.
   *
   *  `panes = []` is what "closed" means to a caller that reads state before the next `open`;
   *  the other four exist only to keep it from being "closed, but still carrying whatever
   *  focus/zoom/pan/panPointer a previous comparison left behind". */
  function reset() {
    panes = [];
    focus = 0;
    zoom = MIN_ZOOM;
    pan = { x: 0, y: 0 };
    panPointer = null;
  }

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
     *  and the panes are laid out by how many there are.
     *
     *  An invalid count (the early return) refuses rather than resetting: a stray `open([x])`
     *  while three photos are already being compared is a no-op, not something that should
     *  discard what the person is looking at. Everything past the guard - a load that
     *  succeeds or fails - does go through `reset()`, because both of those really do replace
     *  whatever comparison was open. */
    async open(ids: number[]) {
      if (!canCompare(ids.length)) return;
      try {
        const loaded = await Promise.all(ids.map((id) => deps.load(id)));
        reset();
        panes = loaded;
      } catch (e) {
        reset();
        deps.onerror(e);
      }
    },

    close() {
      reset();
      deps.onclose();
    },

    /** Zooms by `factor` about a point `originX`/`originY` from the pane's centre.
     *
     *  The pan correction is what keeps that point still, and it has to be derived rather
     *  than guessed at. The picture is drawn `translate(T) scale(z)` about
     *  `transform-origin: center`, so a point `p` of the picture (measured from its centre)
     *  lands at screen offset `T + z*p` from the pane's centre. To hold whatever is under
     *  screen offset `d`:
     *
     *      p  = (d - T0) / z0            what is under the pointer now
     *      T1 = d - z1*p = d - r*(d - T0)      where r = z1/z0
     *
     *  Note the `T0` inside the bracket. `T0 - d*(r - 1)` - the shape this started as - is
     *  the same expression only while the pan is zero; it is off by `T0 * (r - 1)` otherwise,
     *  so the photo slid out from under the pointer on every notch after the first, which is
     *  exactly the feeling that makes a shared zoom useless for comparing. A single-zoom test
     *  from `pan = {0, 0}` cannot see the difference, which is why the test beside it zooms
     *  twice about the same point. */
    zoomAt(factor: number, originX: number, originY: number, width: number, height: number) {
      const next = clampZoom(zoom * factor);
      const k = next / zoom;
      const x = originX - k * (originX - pan.x);
      const y = originY - k * (originY - pan.y);
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

    /** Tab's partner. `+ panes.length` before the modulo because JavaScript's `%` keeps the
     *  sign of its left operand, so `(0 - 1) % 3` is -1 rather than the last pane. */
    prevPane() {
      if (panes.length > 0) focus = (focus - 1 + panes.length) % panes.length;
    },

    /** Whether pane `i` should ask for the full-size render rather than the preview.
     *
     *  At most one pane ever does. Full-size renders are serialised behind `RENDERING` in
     *  `protocol.rs`, so letting every pane upgrade would queue four 24 MP decodes and show
     *  nothing until the last finished; and below 100% the preview's pixels are all that can
     *  be seen anyway. Held by `only the focused pane asks for a full render, and only above
     *  fit` in compare.svelte.test.ts. */
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
