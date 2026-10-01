import { describe, expect, it, vi } from 'vitest';
import { errorMessage, type WatchedFolder } from './api';
import { createFolderDrop } from './folder-drop.svelte';

function setup(refuse: Record<string, string> = {}) {
  let nextId = 1;
  const add = vi.fn(async (path: string): Promise<WatchedFolder> => {
    // As the backend refuses: an object with a kind and a message, not an `Error`.
    if (refuse[path]) throw { kind: 'notAFolder', message: refuse[path] };
    return { id: nextId++, path, online: true };
  });
  const refresh = vi.fn(async () => {});
  const notify = vi.fn((_message: string) => {});
  const reportError = vi.fn((_error: unknown) => {});
  const drop = createFolderDrop({ add, refresh, notify, reportError });
  const reported = () => reportError.mock.calls.map(([e]) => errorMessage(e));
  return { drop, add, refresh, notify, reported };
}

describe('createFolderDrop', () => {
  it('shows the overlay while paths hover, and not for a drag that carries none', async () => {
    const { drop } = setup();
    expect(drop.hovering).toBe(false);
    await drop.handle({ type: 'enter', paths: ['/home/ada/Holiday'] });
    expect(drop.hovering).toBe(true);
    await drop.handle({ type: 'leave' });
    expect(drop.hovering).toBe(false);
    await drop.handle({ type: 'enter', paths: [] });
    expect(drop.hovering).toBe(false);
  });

  it('watches a dropped folder, refreshes once and names it', async () => {
    const { drop, add, refresh, notify, reported } = setup();
    await drop.handle({ type: 'enter', paths: ['/home/ada/Holiday'] });
    await drop.handle({ type: 'drop', paths: ['/home/ada/Holiday'] });
    expect(drop.hovering).toBe(false);
    expect(add).toHaveBeenCalledWith('/home/ada/Holiday');
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(notify).toHaveBeenCalledWith('Watching “Holiday”');
    expect(reported()).toEqual([]);
  });

  it('names a Windows folder by its last component too', async () => {
    const { drop, notify } = setup();
    await drop.handle({ type: 'drop', paths: ['C:\\Users\\ada\\Pictures\\'] });
    expect(notify).toHaveBeenCalledWith('Watching “Pictures”');
  });

  it('counts several folders and still refreshes once', async () => {
    const { drop, refresh, notify } = setup();
    await drop.handle({ type: 'drop', paths: ['/a', '/b', '/c'] });
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(notify).toHaveBeenCalledWith('Watching 3 folders');
  });

  it('adds one at a time, so the backend can refuse a second that overlaps the first', async () => {
    const order: string[] = [];
    let release: (() => void) | undefined;
    const add = vi.fn(async (path: string): Promise<WatchedFolder> => {
      order.push(`start ${path}`);
      if (path === '/a') await new Promise<void>((r) => (release = r));
      order.push(`end ${path}`);
      return { id: 1, path, online: true };
    });
    const drop = createFolderDrop({ add, refresh: async () => {}, notify: () => {}, reportError: () => {} });
    const done = drop.handle({ type: 'drop', paths: ['/a', '/a/b'] });
    await Promise.resolve();
    expect(order).toEqual(['start /a']);
    release?.();
    await done;
    expect(order).toEqual(['start /a', 'end /a', 'start /a/b', 'end /a/b']);
  });

  it('reports what was refused once, and still watches the rest', async () => {
    const { drop, refresh, notify, reported } = setup({
      '/x/a.jpg': 'a.jpg is a file, not a folder',
      '/x/b.jpg': 'b.jpg is a file, not a folder',
      '/x/c.jpg': 'c.jpg is a file, not a folder',
    });
    await drop.handle({ type: 'drop', paths: ['/x/a.jpg', '/x/Holiday', '/x/b.jpg', '/x/c.jpg'] });
    expect(notify).toHaveBeenCalledWith('Watching “Holiday”');
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(reported()).toEqual(['a.jpg is a file, not a folder (and 2 more)']);
  });

  it('says nothing was watched when nothing was', async () => {
    const { drop, refresh, notify, reported } = setup({ '/x/a.jpg': 'a.jpg is a file, not a folder' });
    await drop.handle({ type: 'drop', paths: ['/x/a.jpg'] });
    expect(notify).not.toHaveBeenCalled();
    expect(refresh).not.toHaveBeenCalled();
    expect(reported()).toEqual(['a.jpg is a file, not a folder']);
  });

  it('does nothing for a drop that carries no paths', async () => {
    const { drop, add } = setup();
    await drop.handle({ type: 'drop', paths: [] });
    expect(add).not.toHaveBeenCalled();
  });
});
