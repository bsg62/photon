/** Fixed-size pages of grid rows, fetched on demand and dropped when the grid version changes. */

export const PAGE_SIZE = 200;

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

  reset(version: number): void {
    if (version === this.version) return;
    this.version = version;
    this.pages.clear();
    this.loading.clear();
  }

  get(offset: number): T | undefined {
    return this.pages.get(Math.floor(offset / PAGE_SIZE))?.[offset % PAGE_SIZE];
  }

  /** Loads the missing pages covering `[start, end)`. Resolves true if anything new was stored. */
  async ensure(start: number, end: number): Promise<boolean> {
    const first = Math.floor(start / PAGE_SIZE);
    const last = Math.floor(Math.max(start, end - 1) / PAGE_SIZE);
    const wanted: number[] = [];
    for (let p = first; p <= last; p++) {
      if (!this.pages.has(p) && !this.loading.has(p)) wanted.push(p);
    }
    if (wanted.length === 0) return false;
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
    if (version !== this.version) return false;
    let stored = false;
    for (const [p, page] of results) {
      this.loading.delete(p);
      if (!page) continue;
      if (page.version !== version) {
        this.onStale(page.version);
        continue;
      }
      this.pages.set(p, page.rows);
      stored = true;
    }
    return stored;
  }
}
