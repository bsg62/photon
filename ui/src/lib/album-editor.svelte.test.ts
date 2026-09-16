import { describe, expect, it, vi } from 'vitest';
import { createAlbumEditor } from './album-editor.svelte';

function editor() {
  const create = vi.fn(async (_name: string) => ({}));
  const rename = vi.fn(async (_id: number, _name: string) => {});
  return { create, rename, ed: createAlbumEditor({ create, rename }) };
}

describe('createAlbumEditor', () => {
  it('creates a new album from a trimmed name and returns to idle', async () => {
    const { create, ed } = editor();
    expect(ed.editing()).toBe(false);
    ed.startNew();
    expect(ed.editing()).toBe(true);
    ed.text = '  Trip  ';
    await expect(ed.commit()).resolves.toBe(true);
    expect(create).toHaveBeenCalledWith('Trip');
    expect(ed.editing()).toBe(false);
    expect(ed.text).toBe('');
  });

  it('renames with the field seeded from the current name', async () => {
    const { rename, ed } = editor();
    ed.startRename(7, 'Trip');
    expect(ed.editing(7)).toBe(true);
    expect(ed.editing(8)).toBe(false);
    expect(ed.editing()).toBe(false);
    expect(ed.text).toBe('Trip');
    ed.text = 'Zoo';
    await ed.commit();
    expect(rename).toHaveBeenCalledWith(7, 'Zoo');
    expect(ed.mode.kind).toBe('idle');
  });

  it('treats a blank commit as a cancel, and sends nothing', async () => {
    const { create, rename, ed } = editor();
    ed.startNew();
    ed.text = '   ';
    await expect(ed.commit()).resolves.toBe(false);
    expect(create).not.toHaveBeenCalled();
    expect(rename).not.toHaveBeenCalled();
    expect(ed.mode.kind).toBe('idle');
    await expect(ed.commit()).resolves.toBe(false);
  });

  it('keeps the field open when the backend refuses, so the name can be fixed', async () => {
    const { ed } = editor();
    const failing = createAlbumEditor({
      create: async () => {
        throw new Error('nope');
      },
      rename: async () => {},
    });
    failing.startNew();
    failing.text = 'Trip';
    await expect(failing.commit()).rejects.toThrow('nope');
    expect(failing.editing()).toBe(true);
    expect(failing.text).toBe('Trip');
    expect(failing.busy).toBe(false);
    void ed;
  });

  it('starting a rename replaces a half-typed new album', () => {
    const { ed } = editor();
    ed.startNew();
    ed.text = 'half';
    ed.startRename(3, 'Old');
    expect(ed.editing()).toBe(false);
    expect(ed.editing(3)).toBe(true);
    expect(ed.text).toBe('Old');
  });
});
