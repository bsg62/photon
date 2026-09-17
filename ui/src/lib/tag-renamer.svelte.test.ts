import { describe, expect, it, vi } from 'vitest';
import type { TagCount, TagRule } from './api';
import { createTagRenamer } from './tag-renamer.svelte';

const tags: TagCount[] = [
  { tag: 'holiday', count: 1 },
  { tag: 'vacation', count: 2 },
];
const rules: TagRule[] = [{ tag: 'junk', target: null }];

function renamer(confirmed = true) {
  const rename = vi.fn(async (_from: string, _to: string) => {});
  const confirm = vi.fn(async (_message: string) => confirmed);
  const r = createTagRenamer({ rename, confirm, errorMessage: (e) => String(e) });
  return { rename, confirm, r };
}

describe('createTagRenamer', () => {
  it('renames to the trimmed name and closes the field', async () => {
    const { rename, confirm, r } = renamer();
    r.start('holiday');
    expect(r.editing).toBe('holiday');
    expect(r.text).toBe('holiday');
    r.text = '  trip ';
    await expect(r.commit(tags, rules)).resolves.toBe(true);
    expect(rename).toHaveBeenCalledWith('holiday', 'trip');
    expect(confirm).not.toHaveBeenCalled();
    expect(r.editing).toBeNull();
  });

  it('refuses a blank name and keeps the field open', async () => {
    const { rename, r } = renamer();
    r.start('holiday');
    r.text = '  ';
    await expect(r.commit(tags, rules)).resolves.toBe(false);
    expect(rename).not.toHaveBeenCalled();
    expect(r.editing).toBe('holiday');
    expect(r.error).toBe('A tag needs a name.');
  });

  it('closes without a call when the name is unchanged', async () => {
    const { rename, r } = renamer();
    r.start('holiday');
    r.text = ' holiday ';
    await expect(r.commit(tags, rules)).resolves.toBe(false);
    expect(rename).not.toHaveBeenCalled();
    expect(r.editing).toBeNull();
  });

  it('keeps the typed name when a merge is declined', async () => {
    const { rename, confirm, r } = renamer(false);
    r.start('holiday');
    r.text = 'vacation';
    await expect(r.commit(tags, rules)).resolves.toBe(false);
    expect(confirm).toHaveBeenCalledWith(expect.stringContaining('Merge “holiday” into “vacation”'));
    expect(rename).not.toHaveBeenCalled();
    expect(r.editing).toBe('holiday');
    expect(r.text).toBe('vacation');
  });

  it('asks before renaming onto a name listed under Changes', async () => {
    const { rename, confirm, r } = renamer();
    r.start('holiday');
    r.text = 'junk';
    await expect(r.commit(tags, rules)).resolves.toBe(true);
    expect(confirm).toHaveBeenCalledWith(expect.stringContaining('“junk” is listed under Changes'));
    expect(rename).toHaveBeenCalledWith('holiday', 'junk');
  });

  it('a blur while the confirm is open does not cancel the edit it asks about', async () => {
    let answer!: (ok: boolean) => void;
    const rename = vi.fn(async (_from: string, _to: string) => {});
    const r = createTagRenamer({
      rename,
      confirm: () => new Promise<boolean>((resolve) => (answer = resolve)),
      errorMessage: String,
    });
    r.start('holiday');
    r.text = 'vacation';
    const pending = r.commit(tags, rules);
    expect(r.busy).toBe(true);
    r.cancel();
    expect(r.editing).toBe('holiday');
    expect(r.text).toBe('vacation');
    await expect(r.commit(tags, rules)).resolves.toBe(false);
    answer(true);
    await expect(pending).resolves.toBe(true);
    expect(rename).toHaveBeenCalledTimes(1);
    expect(rename).toHaveBeenCalledWith('holiday', 'vacation');
  });

  it('keeps the typed name and shows the error when the rename fails', async () => {
    const rename = vi.fn(async () => {
      throw new Error('disk full');
    });
    const r = createTagRenamer({ rename, confirm: async () => true, errorMessage: (e) => (e as Error).message });
    r.start('holiday');
    r.text = 'trip';
    await expect(r.commit(tags, rules)).resolves.toBe(false);
    expect(r.editing).toBe('holiday');
    expect(r.text).toBe('trip');
    expect(r.error).toBe('disk full');
    expect(r.busy).toBe(false);

    r.text = 'trips';
    expect(r.error).toBe('');
    r.cancel();
    expect(r.editing).toBeNull();
    expect(r.error).toBe('');
  });
});
