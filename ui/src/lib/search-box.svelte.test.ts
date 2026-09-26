import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createSearchBox, searchBox } from './search-box.svelte';
import { SEARCH_DEBOUNCE_MS } from './search';

// The singleton at the bottom of the module wires itself to the real store on import, which
// would drag the Tauri API in with it. Only the factory is under test here.
// The hook the singleton registers, captured as it registers it: the module runs once, on
// import, before any test does.
const registered = vi.hoisted(() => ({ hooks: [] as (() => () => void)[] }));
vi.mock('./library.svelte', () => ({
  library: {
    setSearchQuery: (q: string) => Promise.resolve(q),
    onViewSwitch: (hook: () => () => void) => {
      registered.hooks.push(hook);
      return () => {};
    },
  },
}));

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

describe('createSearchBox', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('sends the typed query once the debounce window elapses', () => {
    const send = vi.fn((q: string) => Promise.resolve(q));
    const box = createSearchBox(send);

    box.query = 'beach';
    box.run('beach');
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS);

    expect(send).toHaveBeenCalledExactlyOnceWith('beach');
  });

  it('drops a pending send on cancel, so a view switch is not undone by it', () => {
    const send = vi.fn((q: string) => Promise.resolve(q));
    const box = createSearchBox(send);

    box.run('beach');
    box.cancel();
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS * 10);

    expect(send).not.toHaveBeenCalled();
  });

  it('clear empties the box and sends the empty query, without the pending one landing after it', () => {
    const send = vi.fn((q: string) => Promise.resolve(q));
    const box = createSearchBox(send);

    box.query = 'beach';
    box.run('beach');
    box.clear();
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS * 10);

    expect(box.query).toBe('');
    expect(send).toHaveBeenCalledExactlyOnceWith('');
  });

  it('runs a linked search at once and drops what was half-typed before it', () => {
    const send = vi.fn((q: string) => Promise.resolve(q));
    const box = createSearchBox(send);

    box.query = 'bea';
    box.run('bea');
    box.search('camera:"NIKON D750"');
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS * 10);

    // Without the cancel, 'bea' fires after the link's query and replaces it.
    expect(box.query).toBe('camera:"NIKON D750"');
    expect(send).toHaveBeenCalledExactlyOnceWith('camera:"NIKON D750"');
  });

  it('empties the box when the view is left, and drops the send still pending', () => {
    const send = vi.fn((q: string) => Promise.resolve(q));
    const box = createSearchBox(send);

    box.query = 'beach';
    box.run('beach');
    box.leave();
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS * 10);

    // The backend clears its query on a view switch; a box still holding 'beach' would
    // offer a search that no longer filters anything.
    expect(box.query).toBe('');
    expect(send).not.toHaveBeenCalled();
  });

  it('puts the text back when the switch is refused', () => {
    const box = createSearchBox(vi.fn((q: string) => Promise.resolve(q)));

    box.query = 'beach';
    const undo = box.leave();
    undo();

    expect(box.query).toBe('beach');
  });

  it('keeps what was typed after leaving when a refused switch is taken back', () => {
    const box = createSearchBox(vi.fn((q: string) => Promise.resolve(q)));

    box.query = 'beach';
    const undo = box.leave();
    box.query = 'hut';
    undo();

    expect(box.query).toBe('hut');
  });

  it('shows what the backend rolled back to when a search is refused', async () => {
    const box = createSearchBox(() => Promise.resolve('lake'));

    box.search('beach');
    await Promise.resolve();
    await Promise.resolve();

    // The grid is still showing 'lake'; a box left on 'beach' would offer to save a search
    // that filters nothing, and Escape would then move the grid.
    expect(box.query).toBe('lake');
  });

  it('keeps what was typed on after a refused search', async () => {
    let refuse!: (held: string) => void;
    const box = createSearchBox(() => new Promise<string>((res) => (refuse = res)));

    box.search('beach');
    box.query = 'beaches';
    refuse('lake');
    await Promise.resolve();
    await Promise.resolve();

    expect(box.query).toBe('beaches');
  });
});

describe('the search box singleton', () => {
  it('empties itself on every view switch the store issues', () => {
    // The only link between a view switch and the box: without it, clicking Starred during
    // a search leaves the query in the box over Starred's grid.
    expect(registered.hooks).toHaveLength(1);
    searchBox.query = 'beach';
    registered.hooks[0]();
    expect(searchBox.query).toBe('');
  });
});
