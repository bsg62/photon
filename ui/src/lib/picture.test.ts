import { describe, expect, it } from 'vitest';
import { pictureChanged } from './picture';

const shown = { thumbKey: 'aa', width: 40, height: 20, orientation: 1, thumbState: 'ready' as const };

describe('pictureChanged', () => {
  it('is a new key, size or orientation', () => {
    expect(pictureChanged(shown, { ...shown })).toBe(false);
    expect(pictureChanged(shown, { ...shown, thumbKey: 'bb' })).toBe(true);
    expect(pictureChanged(shown, { ...shown, width: 20, height: 40 })).toBe(true);
    expect(pictureChanged(shown, { ...shown, orientation: 6 })).toBe(true);
  });

  it('is not the thumbnail being made', () => {
    // What every edit leaves behind: the viewer holds `pending`, the worker finishes, and
    // an unrelated change delivers `ready`.
    expect(pictureChanged({ ...shown, thumbState: 'pending' }, shown)).toBe(false);
    expect(pictureChanged(shown, { ...shown, thumbState: 'pending' })).toBe(false);
  });

  it('is a photo becoming showable, or ceasing to be', () => {
    expect(pictureChanged({ ...shown, thumbState: 'failed' }, shown)).toBe(true);
    expect(pictureChanged(shown, { ...shown, thumbState: 'failed' })).toBe(true);
    expect(pictureChanged({ ...shown, thumbState: 'failed' }, { ...shown, thumbState: 'failed' })).toBe(false);
  });
});
