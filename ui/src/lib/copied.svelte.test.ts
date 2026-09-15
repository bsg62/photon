import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { COPIED_MS, createCopyFeedback } from './copied.svelte';

describe('createCopyFeedback', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('writes the text and shows the confirmation until it times out', async () => {
    const write = vi.fn(() => Promise.resolve());
    const feedback = createCopyFeedback(write);

    await feedback.copy('IMG_1234.JPG');

    expect(write).toHaveBeenCalledExactlyOnceWith('IMG_1234.JPG');
    expect(feedback.copied).toBe(true);
    vi.advanceTimersByTime(COPIED_MS - 1);
    expect(feedback.copied).toBe(true);
    vi.advanceTimersByTime(1);
    expect(feedback.copied).toBe(false);
  });

  it('a second copy restarts the timer rather than cutting the confirmation short', async () => {
    const feedback = createCopyFeedback(() => Promise.resolve());

    await feedback.copy('a');
    vi.advanceTimersByTime(COPIED_MS - 100);
    await feedback.copy('b');
    vi.advanceTimersByTime(100);

    expect(feedback.copied).toBe(true);
  });

  it('shows nothing and rethrows when the write fails, so the caller can report it', async () => {
    const feedback = createCopyFeedback(() => Promise.reject(new Error('no clipboard')));

    await expect(feedback.copy('a')).rejects.toThrow('no clipboard');
    expect(feedback.copied).toBe(false);
  });

  it('dispose cancels a pending timer', async () => {
    const feedback = createCopyFeedback(() => Promise.resolve());
    await feedback.copy('a');
    feedback.dispose();
    vi.advanceTimersByTime(COPIED_MS);
    // No timer fired after dispose; the flag is simply left as the caller last saw it.
    expect(feedback.copied).toBe(true);
  });
});
