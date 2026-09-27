/** Fixed-size pages of grid rows, fetched on demand and dropped when the grid version changes. */

export const PAGE_SIZE = 200;

/** How far past the grid's window loaded pages are kept, in grid offsets, before `evict`
 *  lets them go: enough that scrolling back a little does not fetch again. */
export const KEEP_SLACK = 5 * PAGE_SIZE;

/** The page holding grid offset `offset`. */
export function pageOf(offset: number): number {
  return Math.floor(offset / PAGE_SIZE);
}

export interface Page<T> {
  version: number;
  rows: T[];
}

type Loader<T> = (offset: number, count: number) => Promise<Page<T>>;

export class PageCache<T> {
  version = -1;
  private readonly load: Loader<T>;
  private readonly onStale: (version: number) => void;
  private pages = new Map<number, T[]>();
  private loading = new Set<number>();

  constructor(load: Loader<T>, onStale: (version: number) => void = () => {}) {
    this.load = load;
    this.onStale = onStale;
  }

  /** Moves to `version`, dropping every page of the old one. `seed` is pages already loaded
   *  at `version` (see `prefetch`), installed in the same step so the rows on screen are
   *  never momentarily missing. */
  reset(version: number, seed?: Map<number, T[]>): void {
    if (version === this.version) return;
    this.version = version;
    this.pages = seed ?? new Map();
    this.loading.clear();
  }

  /** Loads the pages covering `[start, end)` at `version` without touching the current ones,
   *  for `reset` to install. Clearing first and loading after is what made every rebuild
   *  during a scan blank the whole screen of tiles for a round trip. A page answered at any
   *  other version is left out and reported, as `ensure` does. */
  async prefetch(version: number, start: number, end: number): Promise<Map<number, T[]>> {
    const seed = new Map<number, T[]>();
    if (end <= start) return seed;
    const first = Math.floor(start / PAGE_SIZE);
    const last = Math.floor((end - 1) / PAGE_SIZE);
    const wanted: number[] = [];
    for (let p = first; p <= last; p++) wanted.push(p);
    const results = await Promise.all(
      wanted.map((p) =>
        this.load(p * PAGE_SIZE, PAGE_SIZE).then(
          (page) => [p, page] as const,
          () => [p, null] as const,
        ),
      ),
    );
    for (const [p, page] of results) {
      if (!page) continue;
      if (page.version !== version) {
        this.onStale(page.version);
        continue;
      }
      seed.set(p, page.rows);
    }
    return seed;
  }

  get(offset: number): T | undefined {
    return this.pages.get(pageOf(offset))?.[offset % PAGE_SIZE];
  }

  /** How many pages are held: what `evict` keeps bounded. */
  get size(): number {
    return this.pages.size;
  }

  /** Drops every loaded page that touches none of the `keep` ranges (grid offsets,
   *  `[start, end)`), and returns the pages dropped. Within one version nothing else ever
   *  drops a page, so without this a scrub from one end of a large library to the other
   *  held every row of it. A page still loading is left to land; the next call drops it if
   *  it is still out of reach. */
  evict(keep: [number, number][]): number[] {
    const dropped: number[] = [];
    for (const p of this.pages.keys()) {
      const first = p * PAGE_SIZE;
      const end = first + PAGE_SIZE;
      if (!keep.some(([s, e]) => first < e && s < end)) dropped.push(p);
    }
    for (const p of dropped) this.pages.delete(p);
    return dropped;
  }

  /** Loads the missing pages covering `[start, end)`. Resolves with the pages it stored,
   *  for a caller that tells each page's readers apart. */
  async ensure(start: number, end: number): Promise<number[]> {
    const first = Math.floor(start / PAGE_SIZE);
    const last = Math.floor(Math.max(start, end - 1) / PAGE_SIZE);
    const wanted: number[] = [];
    for (let p = first; p <= last; p++) {
      if (!this.pages.has(p) && !this.loading.has(p)) wanted.push(p);
    }
    if (wanted.length === 0) return [];
    const version = this.version;
    for (const p of wanted) this.loading.add(p);
    const results = await Promise.all(
      wanted.map((p) =>
        this.load(p * PAGE_SIZE, PAGE_SIZE).then(
          (page) => [p, page] as const,
          () => [p, null] as const,
        ),
      ),
    );
    if (version !== this.version) return [];
    const stored: number[] = [];
    for (const [p, page] of results) {
      this.loading.delete(p);
      if (!page) continue;
      if (page.version !== version) {
        this.onStale(page.version);
        continue;
      }
      this.pages.set(p, page.rows);
      stored.push(p);
    }
    return stored;
  }
}
