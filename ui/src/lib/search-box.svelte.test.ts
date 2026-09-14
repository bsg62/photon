import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createSearchBox } from './search-box.svelte';
import { SEARCH_DEBOUNCE_MS } from './search';

// The singleton at the bottom of the module wires itself to the real store on import, which
// would drag the Tauri API in with it. Only the factory is under test here.
vi.mock('./library.svelte', () => ({
  library: { info: { searchQuery: '' }, setSearchQuery: () => Promise.resolve() },
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
    const box = createSearchBox(send, 'beach');

    box.run('beach');
    box.clear();
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS * 10);

    expect(box.query).toBe('');
    expect(send).toHaveBeenCalledExactlyOnceWith('');
  });

  it('adopts a query the backend changed on its own — a folder jump clearing search', async () => {
    const send = vi.fn(() => Promise.resolve());
    const box = createSearchBox(send, 'beach');

    box.syncFromBackend('');

    expect(box.query).toBe('');
  });

  it('declines its own echo while the user types ahead of it', async () => {
    const first = deferred();
    const send = vi.fn(() => first.promise);
    const box = createSearchBox(send);

    // 'b' is sent and settles; the user has typed 'each' on top of it meanwhile.
    box.query = 'b';
    box.run('b');
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS);
    box.query = 'beach';
    first.resolve();
    await first.promise;
    await Promise.resolve();

    box.syncFromBackend('b');

    expect(box.query).toBe('beach');
  });

  it('declines any echo while a send is still in flight', async () => {
    const pending = deferred();
    const send = vi.fn(() => pending.promise);
    const box = createSearchBox(send);

    box.query = 'beach';
    box.run('beach');
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS);

    // An echo of an older send arriving while 'beach' is still outstanding.
    box.syncFromBackend('bea');

    expect(box.query).toBe('beach');
    pending.resolve();
  });
});
