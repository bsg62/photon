import { describe, expect, it, vi } from 'vitest';
import { canCompare, createCompare, MAX_PANES, MIN_PANES, type ComparePane } from './compare.svelte';
import { MAX_ZOOM, MIN_ZOOM } from './nav';

function pane(id: number): ComparePane {
  return {
    id,
    fileName: `IMG_${id}.jpg`,
    width: 4000,
    height: 3000,
    takenAt: 1_700_000_000 + id,
    thumbKey: `k${id}`,
    edit: false,
    loaded: false,
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

  it('pan is clamped so the photo keeps covering the pane', async () => {
    const c = createCompare(deps());
    await c.open([1, 2]);
    // At fit, there is nowhere to pan.
    c.panBy(500, 500, 800, 600);
    expect(c.pan).toEqual({ x: 0, y: 0 });
  });

  it('closing tells the caller and empties the panes', async () => {
    const d = deps();
    const c = createCompare(d);
    await c.open([1, 2]);
    c.close();
    expect(d.onclose).toHaveBeenCalledOnce();
    expect(c.panes).toEqual([]);
  });

  it('a load failure closes rather than showing half a comparison', async () => {
    const d = deps({ load: vi.fn(async () => { throw new Error('gone'); }) });
    const c = createCompare(d);
    await c.open([1, 2]);
    expect(d.onerror).toHaveBeenCalled();
    expect(c.panes).toEqual([]);
  });
});
