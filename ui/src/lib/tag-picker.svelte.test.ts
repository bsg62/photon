import { describe, expect, it, vi } from 'vitest';
import { createTagPicker } from './tag-picker.svelte';

function setup(result = { tag: 'beach', count: 3 }) {
  const apply = vi.fn(async (_mode: 'add' | 'remove', _tag: string, _ids: number[]) => result);
  const picker = createTagPicker({ apply });
  return { picker, apply };
}

describe('createTagPicker', () => {
  it('narrows the known keywords without regard to case', () => {
    const { picker } = setup();
    picker.show('add', [1, 2]);
    picker.draft = 'BE';
    expect(picker.suggestions(['Beach', 'sunset', 'beer', 'Umbrella'])).toEqual(['Beach', 'beer']);
  });

  /** What the user typed is what they meant; a substring match that happens to sort earlier
   *  must not sit above it, or Enter and the first row disagree. */
  it('puts an exact match first, then the names that start with what was typed', () => {
    const { picker } = setup();
    picker.show('add', [1]);
    picker.draft = 'sun';
    expect(picker.suggestions(['sunset', 'risingsun', 'sun', 'sunny', 'shotgun'])).toEqual([
      'sun',
      'sunset',
      'sunny',
      'risingsun',
    ]);
  });

  it('shows every keyword while nothing is typed', () => {
    const { picker } = setup();
    picker.show('add', [1]);
    expect(picker.suggestions(['beach', 'sun'])).toEqual(['beach', 'sun']);
  });

  it('writes nothing for a blank name', async () => {
    const { picker, apply } = setup();
    picker.show('add', [1, 2]);
    picker.draft = '   ';
    expect(await picker.submit()).toBe(null);
    expect(apply).not.toHaveBeenCalled();
    expect(picker.visible).toBe(true);
  });

  it('writes nothing when nothing is selected', async () => {
    const { picker, apply } = setup();
    picker.show('add', []);
    picker.draft = 'beach';
    expect(await picker.submit()).toBe(null);
    expect(apply).not.toHaveBeenCalled();
  });

  /** The dialog closes on the first Enter, but a second keystroke can land before the close
   *  has rendered. Two writes would double the toast and, in remove mode, race the reader. */
  it('ignores a second submit while the first is still in flight', async () => {
    let land!: (r: { tag: string; count: number }) => void;
    const apply = vi.fn(
      () => new Promise<{ tag: string; count: number }>((resolve) => (land = resolve)),
    );
    const picker = createTagPicker({ apply });
    picker.show('add', [1, 2, 3]);
    picker.draft = 'beach';

    const first = picker.submit();
    expect(picker.busy).toBe(true);
    expect(await picker.submit()).toBe(null);

    land({ tag: 'beach', count: 3 });
    await first;
    expect(apply).toHaveBeenCalledTimes(1);
    expect(picker.busy).toBe(false);
  });

  it('submits the keyword it was handed rather than the draft', async () => {
    const { picker, apply } = setup();
    picker.show('add', [7]);
    picker.draft = 'be';
    await picker.submit('Beach');
    expect(apply).toHaveBeenCalledWith('add', 'Beach', [7]);
  });

  it('closes once a write has landed, and stays closed on failure', async () => {
    const { picker } = setup();
    picker.show('add', [1]);
    picker.draft = 'beach';
    await picker.submit();
    expect(picker.visible).toBe(false);

    const failing = createTagPicker({
      apply: async () => {
        throw new Error('nope');
      },
    });
    failing.show('add', [1]);
    failing.draft = 'beach';
    await expect(failing.submit()).rejects.toThrow('nope');
    expect(failing.visible).toBe(false);
    expect(failing.busy).toBe(false);
  });

  describe('the message it reports', () => {
    it('names the keyword stored, which a rename rule can change', async () => {
      const { picker } = setup({ tag: 'vacation', count: 12 });
      picker.show('add', Array.from({ length: 12 }, (_, i) => i));
      expect(await picker.submit('holiday')).toBe('Added “vacation” to 12 photos');
    });

    it('says from, not to, when removing, and counts one photo singly', async () => {
      const { picker } = setup({ tag: 'beach', count: 1 });
      picker.show('remove', [4]);
      expect(await picker.submit('beach')).toBe('Removed “beach” from 1 photo');
    });

    /** A selection can outlive its photos, so the backend's count is the truth and the
     *  difference is worth saying rather than swallowing. */
    it('says how many of the selection it reached when some had gone', async () => {
      const { picker } = setup({ tag: 'beach', count: 10 });
      picker.show('add', Array.from({ length: 12 }, (_, i) => i));
      expect(await picker.submit('beach')).toBe('Added “beach” to 10 of 12 photos');
    });
  });
});
