import { flushSync } from 'svelte';
import { describe, expect, it } from 'vitest';
import { mountTiles } from './page-signals.harness.svelte';
import { PageSignals } from './page-signals.svelte';

// Runs in the `client` project (vite.config.ts): these are about which reactions re-run,
// which Svelte's server runtime - the one the other tests get - never does at all.
describe('PageSignals, with effects running', () => {
  it('re-reads a tile when its own page arrives, and not for any other page', () => {
    const signals = new PageSignals();
    const tiles = mountTiles(signals, [3, 9]);
    expect(tiles.reads()).toEqual({ 3: 1, 9: 1 });

    // The tile's own first read made page 3's signal. Svelte does not make a reaction depend
    // on state it created itself, so a signal made there would never be heard from again.
    signals.touch([3]);
    flushSync();
    expect(tiles.reads()).toEqual({ 3: 2, 9: 1 });

    signals.touch([4]);
    flushSync();
    expect(tiles.reads()).toEqual({ 3: 2, 9: 1 });
    tiles.stop();
  });

  it('re-reads every tile on a new grid version, and each still hears its own page after', () => {
    const signals = new PageSignals();
    const tiles = mountTiles(signals, [3, 9]);

    signals.touchAll();
    flushSync();
    expect(tiles.reads()).toEqual({ 3: 2, 9: 2 });

    signals.touch([9]);
    flushSync();
    expect(tiles.reads()).toEqual({ 3: 2, 9: 3 });
    tiles.stop();
  });

  it('can be told about pages from inside an effect without that effect re-running itself', () => {
    const signals = new PageSignals();
    const tiles = mountTiles(signals, []);
    expect(tiles.fetches()).toBe(1);

    tiles.scroll();
    expect(tiles.fetches()).toBe(2);
    tiles.stop();
  });
});
