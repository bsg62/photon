import { describe, expect, it, vi } from 'vitest';
import { createGridSize } from './grid-size.svelte';
import { TILE_WIDTH } from './layout';

function deps(overrides: Partial<Parameters<typeof createGridSize>[0]> = {}) {
  return {
    load: vi.fn(async () => 'medium' as const),
    save: vi.fn(async () => {}),
    onerror: vi.fn(),
    ...overrides,
  };
}

describe('createGridSize', () => {
  it('starts at medium before anything has loaded', () => {
    const size = createGridSize(deps());
    expect(size.size).toBe('medium');
    expect(size.width).toBe(TILE_WIDTH.medium);
  });

  it('adopts the stored size', async () => {
    const size = createGridSize(deps({ load: vi.fn(async () => 'large' as const) }));
    await size.init();
    expect(size.size).toBe('large');
    expect(size.width).toBe(TILE_WIDTH.large);
  });

  it('applies first and saves second', async () => {
    const d = deps();
    const size = createGridSize(d);
    await size.init();
    const pending = size.set('small');
    // The click is answered before the write lands.
    expect(size.size).toBe('small');
    await pending;
    expect(d.save).toHaveBeenCalledWith('small');
  });

  it('keeps the size for this session when the save fails', async () => {
    const d = deps({ save: vi.fn(async () => { throw new Error('disk'); }) });
    const size = createGridSize(d);
    await size.init();
    await size.set('large');
    expect(size.size).toBe('large');
    expect(d.onerror).toHaveBeenCalled();
  });

  // The generation counter, as `createTheme` has: the singleton outlives an App remount.
  it('a load that lands after dispose does not change the size', async () => {
    let release!: (value: 'large') => void;
    const d = deps({ load: vi.fn(() => new Promise<'large'>((r) => (release = r))) });
    const size = createGridSize(d);
    const pending = size.init();
    size.dispose();
    release('large');
    await pending;
    expect(size.size).toBe('medium');
  });

  it('a choice made while the load is in flight wins', async () => {
    let release!: (value: 'large') => void;
    const d = deps({ load: vi.fn(() => new Promise<'large'>((r) => (release = r))) });
    const size = createGridSize(d);
    const pending = size.init();
    await size.set('small');
    release('large');
    await pending;
    expect(size.size).toBe('small');
  });
});
