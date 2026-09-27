import { flushSync } from 'svelte';
import type { PageSignals } from './page-signals.svelte';

/** The reactions `PageSignals` serves, as the app wires them, for
 *  `page-signals.client.test.ts`: runes only compile in a `.svelte.ts` module, and a test
 *  file is not one.
 *
 *  A tile is a template derived that reads its page - the first read of a page happens right
 *  there - and an effect that renders from it; `reads` counts the derived's runs. The fetch
 *  is an effect that tells pages from inside itself, as the grid's `ensure` effect does when
 *  it evicts. */
export function mountTiles(signals: PageSignals, pages: number[]) {
  const reads: Record<number, number> = {};
  let fetches = 0;
  let fetchSpan = $state(0);
  const stop = $effect.root(() => {
    for (const page of pages) {
      reads[page] = 0;
      // Counted here, in the derived, not in the effect below: a derived that re-reads an
      // unchanged entry stops there, so the effect alone would not see a page telling every
      // tile - only the cost of each tile re-reading its entry, which is what this is about.
      const entry = $derived.by(() => {
        reads[page]++;
        signals.track(page);
      });
      $effect(() => {
        void entry;
      });
    }
    $effect(() => {
      void fetchSpan;
      fetches++;
      signals.touch([1000]);
    });
  });
  flushSync();
  return {
    reads: () => ({ ...reads }),
    fetches: () => fetches,
    /** The grid's window moving: the fetch effect runs again. */
    scroll() {
      fetchSpan++;
      flushSync();
    },
    stop,
  };
}
