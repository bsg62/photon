import { describe, expect, it } from 'vitest';
import { fadesIn, INSTANT_LOAD_MS } from './tile-fade';

describe('fadesIn', () => {
  it('shows a thumbnail the browser already holds at once', () => {
    expect(fadesIn(true, 0, false)).toBe(false);
    // Complete when the src was set decides it, however late the load event itself runs.
    expect(fadesIn(true, 500, false)).toBe(false);
  });

  it('shows a thumbnail that loads within a frame or two at once', () => {
    expect(fadesIn(false, 4, false)).toBe(false);
    expect(fadesIn(false, INSTANT_LOAD_MS, false)).toBe(false);
  });

  it('fades in a thumbnail that arrives late onto a still grid', () => {
    expect(fadesIn(false, INSTANT_LOAD_MS + 1, false)).toBe(true);
    expect(fadesIn(false, 2000, false)).toBe(true);
  });

  it('shows a late thumbnail at once while the grid scrolls', () => {
    // Loaded in the lead, ahead of the scroll: a fade would still be running as it comes on.
    expect(fadesIn(false, 80, true)).toBe(false);
    expect(fadesIn(false, 2000, true)).toBe(false);
  });
});
