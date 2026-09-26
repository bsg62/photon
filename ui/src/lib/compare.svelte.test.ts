import { describe, expect, it, vi } from 'vitest';
import {
  canCompare,
  createCompare,
  differingFacts,
  MAX_PANES,
  MIN_PANES,
  type ComparePane,
} from './compare.svelte';
import { MAX_ZOOM, MIN_ZOOM } from './nav';

function pane(id: number): ComparePane {
  return {
    id,
    fileName: `IMG_${id}.jpg`,
    width: 4000,
    height: 3000,
    takenAt: 1_700_000_000 + id,
    thumbKey: `k${id}`,
    orientation: 1,
    edit: false,
    kind: 'image',
  };
}

function deps(overrides: Partial<Parameters<typeof createCompare>[0]> = {}) {
  return {
    load: vi.fn(async (id: number) => pane(id)),
    onclose: vi.fn(),
    onerror: vi.fn(),
    ...overrides,
  };
}

describe('canCompare', () => {
  it('accepts two to four and refuses anything else', () => {
    expect(canCompare(1)).toBe(false);
    expect(canCompare(MIN_PANES)).toBe(true);
    expect(canCompare(3)).toBe(true);
    expect(canCompare(MAX_PANES)).toBe(true);
    expect(canCompare(5)).toBe(false);
    expect(canCompare(0)).toBe(false);
  });
});

describe('createCompare', () => {
  it('opens with one pane per id, the first focused', async () => {
    const c = createCompare(deps());
    await c.open([7, 8, 9]);
    expect(c.panes.map((p) => p.id)).toEqual([7, 8, 9]);
    expect(c.focus).toBe(0);
    expect(c.zoom).toBe(MIN_ZOOM);
  });

  it('refuses to open with one or five', async () => {
    const d = deps();
    const c = createCompare(d);
    await c.open([7]);
    expect(c.panes).toEqual([]);
    await c.open([1, 2, 3, 4, 5]);
    expect(c.panes).toEqual([]);
    expect(d.load).not.toHaveBeenCalled();
  });

  /** A stray invalid `open()` - a keypress or a stale caller passing the wrong count - is a
   *  no-op, not a reason to discard whatever comparison is already open: refusing does not
   *  mean resetting. */
  it('an invalid open while comparing leaves the current comparison alone', async () => {
    const c = createCompare(deps());
    await c.open([1, 2, 3]);
    c.focusPane(2);
    c.zoomAt(2, 0, 0, 800, 600);
    await c.open([9]);
    expect(c.panes.map((p) => p.id)).toEqual([1, 2, 3]);
    expect(c.focus).toBe(2);
    expect(c.zoom).toBe(2);
  });

  /** The whole feature: one zoom, read by every pane. Fails the moment zoom is stored per
   *  pane, which is the shape this would drift into. */
  it('zoom is shared across panes', async () => {
    const c = createCompare(deps());
    await c.open([1, 2]);
    c.zoomAt(2, 0, 0, 800, 600);
    expect(c.zoom).toBe(2);
    // There is exactly one zoom; no pane carries its own.
    for (const p of c.panes) expect('zoom' in p).toBe(false);
  });

  it('zoom is clamped to the viewer\'s range', async () => {
    const c = createCompare(deps());
    await c.open([1, 2]);
    c.zoomAt(100, 0, 0, 800, 600);
    expect(c.zoom).toBe(MAX_ZOOM);
    c.zoomAt(0.001, 0, 0, 800, 600);
    expect(c.zoom).toBe(MIN_ZOOM);
  });

  /** Zooming about a point keeps that point still: the pan must move by the same fraction
   *  of the origin offset that the scale changed by. */
  it('zoom about a point keeps that point still', async () => {
    const c = createCompare(deps());
    await c.open([1, 2]);
    // A point 100px right and 50px below the centre, zooming 1 -> 2.
    c.zoomAt(2, 100, 50, 800, 600);
    expect(c.pan.x).toBeCloseTo(-100, 5);
    expect(c.pan.y).toBeCloseTo(-50, 5);
  });

  /** The case above cannot discriminate the formula: from `pan = {0, 0}` the wrong
   *  correction and the right one agree exactly. A second notch about the *same* point is
   *  where they part - `T1 = d - r*(d - T0)` gives -300, and the `T0 - d*(r - 1)` shape the
   *  branch shipped with gives -200, so the point the person is zooming into walks away
   *  under the pointer. With the real 1.15 wheel factor the error is `pan * 0.15` a notch
   *  and compounds smoothly, which is why it read as sloppiness rather than as a bug. */
  it('a second zoom about the same point keeps it still too', async () => {
    const c = createCompare(deps());
    await c.open([1, 2]);
    c.zoomAt(2, 100, 50, 800, 600);
    c.zoomAt(2, 100, 50, 800, 600);
    expect(c.zoom).toBe(4);
    expect(c.pan.x).toBeCloseTo(-300, 5);
    expect(c.pan.y).toBeCloseTo(-150, 5);
  });

  it('pan is clamped so the photo keeps covering the pane', async () => {
    const c = createCompare(deps());
    await c.open([1, 2]);
    // At fit, there is nowhere to pan.
    c.panBy(500, 500, 800, 600);
    expect(c.pan).toEqual({ x: 0, y: 0 });
  });

  it('closing tells the caller, empties the panes and resets focus', async () => {
    const d = deps();
    const c = createCompare(d);
    await c.open([1, 2, 3]);
    c.focusPane(2);
    c.close();
    expect(d.onclose).toHaveBeenCalledOnce();
    expect(c.panes).toEqual([]);
    // Task 1's review found this stale: focus left at 2 against an empty `panes` array.
    expect(c.focus).toBe(0);
  });

  it('a load failure closes rather than showing half a comparison', async () => {
    const d = deps({ load: vi.fn(async () => { throw new Error('gone'); }) });
    const c = createCompare(d);
    await c.open([1, 2]);
    expect(d.onerror).toHaveBeenCalled();
    expect(c.panes).toEqual([]);
  });

  it('focus moves by index and wraps', async () => {
    const c = createCompare(deps());
    await c.open([1, 2, 3]);
    c.focusPane(2);
    expect(c.focus).toBe(2);
    c.nextPane();
    expect(c.focus).toBe(0);
    // Out of range is ignored rather than throwing: `3` is a key a person can press.
    c.focusPane(9);
    expect(c.focus).toBe(0);
  });

  // Shift+Tab is a backwards Tab: a roving focus that answers it by moving forwards is
  // wrong in a way that only a wrap-around case catches, so this one starts at 0.
  it('focus moves backwards and wraps the other way', async () => {
    const c = createCompare(deps());
    await c.open([1, 2, 3]);
    c.prevPane();
    expect(c.focus).toBe(2);
    c.prevPane();
    expect(c.focus).toBe(1);
  });

  it('moving focus backwards with nothing open does nothing', async () => {
    const c = createCompare(deps());
    c.prevPane();
    expect(c.focus).toBe(0);
  });

  /** Fails if the upgrade rule is dropped - the change that would quietly serialise four
   *  24 MP renders behind `RENDERING`. */
  it('only the focused pane asks for a full render, and only above fit', async () => {
    const c = createCompare(deps());
    await c.open([1, 2, 3]);
    // At fit, nobody needs one.
    expect([0, 1, 2].map((i) => c.needsFullImage(i))).toEqual([false, false, false]);
    c.zoomAt(2, 0, 0, 800, 600);
    expect([0, 1, 2].map((i) => c.needsFullImage(i))).toEqual([true, false, false]);
    c.focusPane(2);
    expect([0, 1, 2].map((i) => c.needsFullImage(i))).toEqual([false, false, true]);
    // At most one, always.
    expect([0, 1, 2].filter((i) => c.needsFullImage(i))).toHaveLength(1);
  });

  // The three endings - pointerup, Escape, pointercancel - live in the component (Task 3),
  // which has no test harness here; this factory has exactly one teardown, reachable
  // repeatedly, which is the property that matters at this layer. Looping over three labels
  // that all call the same `endPan` would be decorative, so this checks the one path twice
  // instead of pretending to check three.
  /** The bug class CLAUDE.md records from the rubber band: a drag that ends any way but
   *  pointerup must still clear the pointer id, or every later drag is blocked. */
  it('endPan is one teardown, reusable for a second gesture', async () => {
    const c = createCompare(deps());
    await c.open([1, 2]);
    c.beginPan(7);
    expect(c.panning).toBe(true);
    c.endPan();
    expect(c.panning).toBe(false);
    // A second gesture must be able to start after the first ended, whichever way.
    c.beginPan(8);
    expect(c.panning).toBe(true);
    c.endPan();
    expect(c.panning).toBe(false);
  });

  it('closing while panning leaves no live gesture', async () => {
    const c = createCompare(deps());
    await c.open([1, 2]);
    c.beginPan(7);
    c.close();
    expect(c.panning).toBe(false);
  });

  // Consistent with `open`'s own failure branch resetting zoom/pan/focus together (Task 1):
  // a gesture left over from a previous comparison must not survive into the next one either.
  it('opening a new comparison while panning leaves no live gesture', async () => {
    const c = createCompare(deps());
    await c.open([1, 2]);
    c.beginPan(7);
    await c.open([3, 4]);
    expect(c.panning).toBe(false);
  });
});

describe('differingFacts', () => {
  function sized(id: number, w: number, h: number, takenAt: number): ComparePane {
    return { ...pane(id), width: w, height: h, takenAt };
  }

  it('says nothing when every pane carries the same facts', () => {
    const facts = differingFacts([sized(1, 4000, 3000, 100), sized(2, 4000, 3000, 100)], 'en-US');
    expect(facts).toEqual([
      { size: '', taken: '' },
      { size: '', taken: '' },
    ]);
  });

  it('reports each pane’s dimensions once any pane differs', () => {
    const facts = differingFacts([sized(1, 4000, 3000, 100), sized(2, 2000, 1500, 100)], 'en-US');
    expect(facts.map((f) => f.size)).toEqual(['4000 × 3000', '2000 × 1500']);
    expect(facts.map((f) => f.taken)).toEqual(['', '']);
  });

  it('reports each pane’s capture time once any pane differs', () => {
    const facts = differingFacts(
      [sized(1, 4000, 3000, 1_718_454_645), sized(2, 4000, 3000, 1_718_454_700)],
      'en-US',
    );
    expect(facts.map((f) => f.size)).toEqual(['', '']);
    expect(facts.map((f) => f.taken)).toEqual(['Jun 15, 2024, 12:30 PM', 'Jun 15, 2024, 12:31 PM']);
  });

  // A fact is shown on every pane or none: "4000 × 3000" next to a blank says nothing about
  // the blank one, and the comparison is what the person came for.
  it('reports a fact on every pane when only one of three differs', () => {
    const facts = differingFacts(
      [sized(1, 4000, 3000, 100), sized(2, 4000, 3000, 100), sized(3, 4000, 2250, 100)],
      'en-US',
    );
    expect(facts.map((f) => f.size)).toEqual(['4000 × 3000', '4000 × 3000', '4000 × 2250']);
  });

  /** `viewer_item` normalises `width`/`height` only for an *edited* photo, so an ordinary
   *  portrait frame arrives as its stored landscape pair with `orientation: 6`. Comparing
   *  the raw numbers made a portrait and a landscape from the same camera "the same size",
   *  which printed nothing on either pane - suppressing the one fact that actually told them
   *  apart - and printed a size the viewer's own caption contradicted. */
  it('swaps the dimensions of a quarter-turned photo, in the comparison and in the text', () => {
    const portrait = { ...sized(1, 4000, 3000, 100), orientation: 6 };
    const landscape = sized(2, 4000, 3000, 100);
    const facts = differingFacts([portrait, landscape], 'en-US');
    expect(facts.map((f) => f.size)).toEqual(['3000 × 4000', '4000 × 3000']);
  });

  // The same height at a different width is a different picture, so both numbers count.
  it('notices a difference in width alone', () => {
    const facts = differingFacts([sized(1, 4000, 3000, 100), sized(2, 3000, 3000, 100)], 'en-US');
    expect(facts.map((f) => f.size)).toEqual(['4000 × 3000', '3000 × 3000']);
  });

  it('has nothing to compare with fewer than two panes', () => {
    expect(differingFacts([sized(1, 4000, 3000, 100)], 'en-US')).toEqual([{ size: '', taken: '' }]);
    expect(differingFacts([], 'en-US')).toEqual([]);
  });
});
