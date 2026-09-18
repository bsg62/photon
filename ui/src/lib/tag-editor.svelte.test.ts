import { describe, expect, it, vi } from 'vitest';
import { createTagEditor } from './tag-editor.svelte';

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe('createTagEditor', () => {
  it('shows a tag optimistically and calls add for the bound photo', async () => {
    const add = vi.fn(async (_id: number, tag: string) => tag);
    const editor = createTagEditor({ add, remove: async () => {} });
    editor.bind(7, ['beach']);

    const p = editor.add('  sunset ');
    expect(editor.list).toEqual(['beach', 'sunset']);
    expect(editor.busy('sunset')).toBe(true);
    await p;
    expect(add).toHaveBeenCalledWith(7, 'sunset');
    expect(editor.busy('sunset')).toBe(false);
  });

  it('shows the name the backend stored when a rule renamed it', async () => {
    const editor = createTagEditor({
      add: async () => 'vacation',
      remove: async () => {},
    });
    editor.bind(7, []);
    await editor.add('holiday');
    expect(editor.list).toEqual(['vacation']);
  });

  it('drops a blank tag and one the photo already carries', async () => {
    const add = vi.fn(async (_id: number, tag: string) => tag);
    const editor = createTagEditor({ add, remove: async () => {} });
    editor.bind(7, ['beach']);
    await editor.add('   ');
    await editor.add('beach');
    expect(add).not.toHaveBeenCalled();
    expect(editor.list).toEqual(['beach']);
  });

  it('removes optimistically and puts the tag back where it was on failure', async () => {
    const editor = createTagEditor({
      add: async (_id, tag) => tag,
      remove: async () => {
        throw new Error('nope');
      },
    });
    editor.bind(7, ['beach', 'sunset', 'dusk']);
    const p = editor.remove('sunset');
    expect(editor.list).toEqual(['beach', 'dusk']);
    await expect(p).rejects.toThrow('nope');
    expect(editor.list).toEqual(['beach', 'sunset', 'dusk']);
  });

  it('reverts only if the same photo is still bound', async () => {
    const gate = deferred<string>();
    const editor = createTagEditor({ add: () => gate.promise, remove: async () => {} });
    editor.bind(7, []);
    const p = editor.add('sunset');
    expect(editor.list).toEqual(['sunset']);

    // The user moved on; the failure must not edit the next photo's tags.
    editor.bind(8, ['sunset']);
    gate.reject(new Error('nope'));
    await expect(p).rejects.toThrow('nope');
    expect(editor.list).toEqual(['sunset']);
  });

  it('drops a second click on a tag whose call is still in flight', async () => {
    const gate = deferred<void>();
    const remove = vi.fn(() => gate.promise);
    const editor = createTagEditor({ add: async (_id, tag) => tag, remove });
    editor.bind(7, ['beach']);
    const first = editor.remove('beach');
    await editor.remove('beach');
    expect(remove).toHaveBeenCalledTimes(1);
    gate.resolve();
    await first;
  });

  it('suggests tags the photo does not have, matched case-insensitively', () => {
    const editor = createTagEditor({ add: async (_id, tag) => tag, remove: async () => {} });
    editor.bind(7, ['Beach']);
    editor.draft = 'be';
    expect(editor.suggestions(['Beach', 'Bergen', 'sunset'])).toEqual(['Bergen']);
    editor.draft = '';
    expect(editor.suggestions(['Beach', 'sunset'])).toEqual(['sunset']);
  });
});
