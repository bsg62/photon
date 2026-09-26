import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createSearchBox } from './search-box.svelte';
import { SEARCH_DEBOUNCE_MS } from './search';

// The singleton at the bottom of the module wires itself to the real store on import, which
// would drag the Tauri API in with it. Only the factory is under test here.
vi.mock('./library.svelte', () => ({
  library: { setSearchQuery: () => Promise.resolve(), onViewSwitch: () => () => {} },
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
    const send = vi.fn(() => Promise.resolve());
    const box = createSearchBox(send);

    box.query = 'beach';
    box.run('beach');
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS);

    expect(send).toHaveBeenCalledExactlyOnceWith('beach');
  });

  it('drops a pending send on cancel, so a view switch is not undone by it', () => {
    const send = vi.fn(() => Promise.resolve());
    const box = createSearchBox(send);

    box.run('beach');
    box.cancel();
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS * 10);

    expect(send).not.toHaveBeenCalled();
  });

  it('clear empties the box and sends the empty query, without the pending one landing after it', () => {
    const send = vi.fn(() => Promise.resolve());
    const box = createSearchBox(send);

    box.query = 'beach';
    box.run('beach');
    box.clear();
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS * 10);

    expect(box.query).toBe('');
    expect(send).toHaveBeenCalledExactlyOnceWith('');
  });

  it('runs a linked search at once and drops what was half-typed before it', () => {
    const send = vi.fn(() => Promise.resolve());
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
    const send = vi.fn(() => Promise.resolve());
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
    const box = createSearchBox(vi.fn(() => Promise.resolve()));

    box.query = 'beach';
    const undo = box.leave();
    undo();

    expect(box.query).toBe('beach');
  });

  it('keeps what was typed after leaving when a refused switch is taken back', () => {
    const box = createSearchBox(vi.fn(() => Promise.resolve()));

    box.query = 'beach';
    const undo = box.leave();
    box.query = 'hut';
    undo();

    expect(box.query).toBe('hut');
  });
});
