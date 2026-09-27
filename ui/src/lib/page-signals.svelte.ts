import { untrack } from 'svelte';

/** One reactive signal per page of the grid, so a page arriving re-runs only what read an
 *  offset on that page.
 *
 *  A single counter for the whole cache re-ran every live tile's `entry` - and, before its
 *  effect keyed on the grid version, every tile's broken-retry check - for each page that
 *  landed, including pages the grid had scrolled away from long before they answered.
 *
 *  The per-page counters are the properties of one `$state` object, not a signal made per
 *  page on first read. Readers are tiles' template deriveds, and Svelte does not make a
 *  reaction depend on state that reaction itself created: a tile whose read made its page's
 *  signal would never hear that page arrive. A property of a state proxy is given its signal
 *  by the proxy, which does not count as the reader creating it.
 *
 *  `touchAll` replaces the object, which every reader read on its way to its property, so all
 *  of them re-run - and the object holds only pages touched since the grid last changed
 *  version.
 *
 *  Tested in the `client` vitest project (`page-signals.client.test.ts`): under the server
 *  runtime the rest of the suite gets, nothing ever re-runs, and both rules above pass. */
export class PageSignals {
  private ticks = $state<Record<number, number>>({});

  /** Makes the running reaction depend on `page`. */
  track(page: number): void {
    void this.ticks[page];
  }

  /** `pages` changed: re-runs whatever read them.
   *
   *  Untracked, because the increment reads the counter it writes: the grid calls this from
   *  inside its fetch effect (`LibraryStore.ensure` evicting), which would otherwise come to
   *  depend on the counter and re-run itself on its own write. */
  touch(pages: Iterable<number>): void {
    untrack(() => {
      for (const p of pages) this.ticks[p] = (this.ticks[p] ?? 0) + 1;
    });
  }

  /** Every page changed at once - the cache was replaced by another version's. */
  touchAll(): void {
    this.ticks = {};
  }
}
