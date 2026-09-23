import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { debounce, resultsChanged, shouldAdoptBackendQuery, viewKey } from './search';

describe('debounce', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('collapses several calls inside the window into one, with the last arguments', () => {
    const fn = vi.fn();
    const debounced = debounce(fn, 150);

    debounced('a');
    vi.advanceTimersByTime(50);
    debounced('b');
    vi.advanceTimersByTime(50);
    debounced('c');
    vi.advanceTimersByTime(150);

    expect(fn).toHaveBeenCalledTimes(1);
    expect(fn).toHaveBeenCalledWith('c');
  });

  it('fires again for a call made after the window has elapsed', () => {
    const fn = vi.fn();
    const debounced = debounce(fn, 150);

    debounced('a');
    vi.advanceTimersByTime(150);
    expect(fn).toHaveBeenCalledTimes(1);

    debounced('b');
    vi.advanceTimersByTime(150);
    expect(fn).toHaveBeenCalledTimes(2);
    expect(fn).toHaveBeenLastCalledWith('b');
  });

  it('cancel() stops a pending call from ever firing', () => {
    const fn = vi.fn();
    const debounced = debounce(fn, 150);

    debounced('a');
    debounced.cancel();
    vi.advanceTimersByTime(500);

    expect(fn).not.toHaveBeenCalled();
  });

  it('is reusable after cancel(): a later call still debounces and fires normally', () => {
    // Pins the `timer = undefined` reset inside `cancel()` — without it, a call made after
    // cancelling would see a stale timer handle and either fail to schedule or clear the
    // wrong thing, breaking the "clear the box then type a new query" sequence.
    const fn = vi.fn();
    const debounced = debounce(fn, 150);

    debounced('a');
    debounced.cancel();

    debounced('b');
    vi.advanceTimersByTime(150);

    expect(fn).toHaveBeenCalledTimes(1);
    expect(fn).toHaveBeenCalledWith('b');
  });
});

describe('shouldAdoptBackendQuery', () => {
  it('declines while a send is outstanding, even when the value differs — this is the blocker', () => {
    // library.setSearchQuery serialises calls, so a second send can already be queued
    // behind a first that hasn't settled. The first send's echo can then arrive while a
    // later value is already in flight; adopting it here would snap the box backwards to
    // that stale value while the correct one is still on its way.
    expect(shouldAdoptBackendQuery('b', 'beach', 'beach', 1)).toBe(false);
  });

  it('adopts once the outstanding count reaches zero', () => {
    expect(shouldAdoptBackendQuery('', 'beach', 'beach', 0)).toBe(true);
  });

  it('declines a value that has not changed, even with nothing outstanding', () => {
    expect(shouldAdoptBackendQuery('beach', 'beach', null, 0)).toBe(false);
  });
});

describe('resultsChanged', () => {
  it('is true when only the view differs', () => {
    expect(resultsChanged({ view: 'all', query: '' }, { view: 'starred', query: '' })).toBe(true);
  });

  it('is true when only the query differs', () => {
    expect(resultsChanged({ view: 'search', query: 'a' }, { view: 'search', query: 'ab' })).toBe(true);
  });

  it('is false when neither the view nor the query differs', () => {
    expect(resultsChanged({ view: 'search', query: 'a' }, { view: 'search', query: 'a' })).toBe(false);
  });
});

describe('shouldAdoptBackendQuery — the reported swallowing bug', () => {
  it('declines our own echo once it settles, while the user has typed ahead of it', () => {
    // The reported symptom: "the search input swallows inputs while typing". Type "b", the
    // debounce fires and sends it; keep typing "each" so the box reads "beach". The send
    // settles, `outstanding` drops to 0 and the effect wakes to a backend holding "b".
    // Nothing is in flight and "b" differs from "beach", so a count-only guard adopts it and
    // "each" vanishes. This is the case that shipped broken.
    expect(shouldAdoptBackendQuery('b', 'beach', 'b', 0)).toBe(false);
  });

  it('still adopts a clear that came from somewhere else', () => {
    // Clicking a folder or Starred clears the query server-side. That value is not one we
    // sent, so it must reach the box — otherwise it keeps displaying text filtering nothing,
    // which is the whole reason the sync exists.
    expect(shouldAdoptBackendQuery('', 'beach', 'beach', 0)).toBe(true);
  });

  it('declines everything while a send is still in flight', () => {
    // The ordering half: an echo arriving while another send is queued behind it may belong
    // to a since-superseded value.
    expect(shouldAdoptBackendQuery('b', 'beach', 'beach', 1)).toBe(false);
  });
});

describe('viewKey', () => {
  const base = { searchQuery: '', person: null, album: null, tag: null, copiesOf: null };

  it('takes the argument that belongs to the active view', () => {
    expect(viewKey({ ...base, view: 'search', searchQuery: 'lake' })).toEqual({ view: 'search', query: 'lake' });
    expect(viewKey({ ...base, view: 'person', person: 'abc' })).toEqual({ view: 'person', query: 'abc' });
    expect(viewKey({ ...base, view: 'album', album: 7 })).toEqual({ view: 'album', query: '7' });
    expect(viewKey({ ...base, view: 'tag', tag: 'beach' })).toEqual({ view: 'tag', query: 'beach' });
    expect(viewKey({ ...base, view: 'copies', copiesOf: { id: 42, fileName: 'a.jpg', gone: false, hidden: false } })).toEqual({
      view: 'copies',
      query: '42',
    });
    expect(viewKey({ ...base, view: 'all' })).toEqual({ view: 'all', query: '' });
  });

  it('so switching albums resets the scroll, and one album re-published does not', () => {
    expect(resultsChanged(viewKey({ ...base, view: 'album', album: 1 }), viewKey({ ...base, view: 'album', album: 2 }))).toBe(true);
    expect(resultsChanged(viewKey({ ...base, view: 'album', album: 1 }), viewKey({ ...base, view: 'album', album: 1 }))).toBe(false);
  });

  it('so switching Copies anchors resets the scroll', () => {
    const a = viewKey({ ...base, view: 'copies', copiesOf: { id: 1, fileName: 'a.jpg', gone: false, hidden: false } });
    const b = viewKey({ ...base, view: 'copies', copiesOf: { id: 2, fileName: 'b.jpg', gone: false, hidden: false } });
    expect(resultsChanged(a, b)).toBe(true);
  });
});
