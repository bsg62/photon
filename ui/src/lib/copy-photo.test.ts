import { describe, expect, it } from 'vitest';
import { isCopyPhotoShortcut } from './copy-photo';

const key = (k: string, mods: Partial<{ ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean }> = {}) => ({
  key: k,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  ...mods,
});

describe('isCopyPhotoShortcut', () => {
  it('is Ctrl+C or Cmd+C, either case', () => {
    expect(isCopyPhotoShortcut(key('c', { ctrlKey: true }), false)).toBe(true);
    expect(isCopyPhotoShortcut(key('C', { metaKey: true }), false)).toBe(true);
  });

  it('is not a plain C (the viewer crops on it), nor with Shift or Alt', () => {
    expect(isCopyPhotoShortcut(key('c'), false)).toBe(false);
    expect(isCopyPhotoShortcut(key('c', { ctrlKey: true, shiftKey: true }), false)).toBe(false);
    expect(isCopyPhotoShortcut(key('c', { ctrlKey: true, altKey: true }), false)).toBe(false);
  });

  it('leaves Ctrl+C to the webview while text is selected', () => {
    // A caption or a path selected in the info panel: the user is copying that text.
    expect(isCopyPhotoShortcut(key('c', { ctrlKey: true }), true)).toBe(false);
  });
});
