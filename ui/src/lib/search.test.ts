import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { debounce, resultsChanged } from './search';

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
