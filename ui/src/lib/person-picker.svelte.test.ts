import { describe, expect, it, vi } from 'vitest';
import type { NamedItems } from './api';
import { createPersonPicker } from './person-picker.svelte';

const none = { items: [], count: 0 };
function named(over: Partial<NamedItems> = {}): NamedItems {
  return { person: 3, name: 'Anna', named: 3, already: none, several: none, rejected: none, none, ...over };
}

function setup() {
  const nameFaces = vi.fn(async (_f: number[], _n: string): Promise<number | null> => 3);
  const nameItems = vi.fn(async (_i: number[], _n: string) => named());
  const nameOf = vi.fn((_p: number): string | undefined => undefined);
  return { picker: createPersonPicker({ nameFaces, nameItems, nameOf }), nameFaces, nameItems, nameOf };
}

describe('createPersonPicker', () => {
  it('show captures the target and clears the draft', () => {
    const { picker } = setup();
    picker.draft = 'old';
    const items = [1, 2];
    picker.show({ kind: 'items', items });
    items.push(3);
    expect(picker.visible).toBe(true);
    expect(picker.draft).toBe('');
    expect(picker.target).toEqual({ kind: 'items', items: [1, 2] });
  });

  it('the title says what it acts on', () => {
    const { picker } = setup();
    picker.show({ kind: 'face', face: 4 });
    expect(picker.title).toBe('Who is this?');
    picker.show({ kind: 'items', items: [1] });
    expect(picker.title).toBe('Add 1 photo to…');
    picker.show({ kind: 'items', items: [1, 2, 3] });
    expect(picker.title).toBe('Add 3 photos to…');
  });

  it('suggestions narrow and rank like the keyword dialog', () => {
    const { picker } = setup();
    picker.show({ kind: 'face', face: 1 });
    const people = [
      { id: 1, name: 'Hannah' },
      { id: 2, name: 'Anna' },
      { id: 3, name: 'Annabel' },
      { id: 4, name: 'Ben' },
      { id: 5, name: 'Émile' },
    ];
    picker.draft = 'ANNA';
    expect(picker.suggestions(people).map((p) => p.id)).toEqual([2, 3, 1]);
    picker.draft = 'émile';
    expect(picker.suggestions(people).map((p) => p.id)).toEqual([5]);
    picker.draft = '';
    expect(picker.suggestions(people)).toHaveLength(5);
  });

  it('the hint says whether Enter adds or creates', () => {
    const { picker } = setup();
    picker.show({ kind: 'face', face: 1 });
    const people = [{ id: 3, name: 'Anna' }];
    picker.draft = 'anna';
    expect(picker.hint(people)).toBe('Add to Anna');
    picker.draft = ' Ben ';
    expect(picker.hint(people)).toBe('New person “Ben”');
    picker.draft = '  ';
    expect(picker.hint(people)).toBe('');
  });

  it('submit closes before the write and names the face', async () => {
    const { picker, nameFaces } = setup();
    let release!: (v: number | null) => void;
    nameFaces.mockImplementation(() => new Promise((r) => (release = r)));
    picker.show({ kind: 'face', face: 7 });
    picker.draft = '  Anna ';
    const line = picker.submit();
    expect(picker.visible).toBe(false);
    expect(nameFaces).toHaveBeenCalledWith([7], 'Anna');
    release(3);
    expect(await line).toBe('This is Anna.');
    expect(picker.busy).toBe(false);
  });

  it('the face toast uses the stored spelling, else the typed name', async () => {
    const { picker, nameOf } = setup();
    nameOf.mockImplementation((p) => (p === 3 ? 'Anna' : undefined));
    picker.show({ kind: 'face', face: 1 });
    expect(await picker.submit('anna')).toBe('This is Anna.');
    expect(nameOf).toHaveBeenCalledWith(3);
    nameOf.mockReturnValue(undefined);
    picker.show({ kind: 'face', face: 1 });
    expect(await picker.submit(' Ben ')).toBe('This is Ben.');
  });

  it('a show during a write does not allow a second write', async () => {
    const { picker, nameFaces } = setup();
    let release!: (v: number | null) => void;
    nameFaces.mockImplementation(() => new Promise((r) => (release = r)));
    picker.show({ kind: 'face', face: 1 });
    const first = picker.submit('Anna');
    picker.show({ kind: 'face', face: 2 });
    expect(await picker.submit('Anna')).toBeNull();
    release(1);
    await first;
    expect(nameFaces).toHaveBeenCalledTimes(1);
  });

  it('submit for photos returns the grid toast line', async () => {
    const { picker, nameItems } = setup();
    picker.show({ kind: 'items', items: [1, 2, 3] });
    expect(await picker.submit('Anna')).toBe('Added 3 photos to Anna.');
    expect(nameItems).toHaveBeenCalledWith([1, 2, 3], 'Anna');
  });

  it('a blank or busy submit does nothing', async () => {
    const { picker, nameFaces } = setup();
    picker.show({ kind: 'face', face: 1 });
    picker.draft = ' ';
    expect(await picker.submit()).toBeNull();
    let release!: (v: number | null) => void;
    nameFaces.mockImplementation(() => new Promise((r) => (release = r)));
    const first = picker.submit('Anna');
    expect(await picker.submit('Anna')).toBeNull();
    release(1);
    await first;
    expect(nameFaces).toHaveBeenCalledTimes(1);
  });

  it('the dialog swallows every key it receives', () => {
    const { picker } = setup();
    const press = (key: string) => {
      const e = { key, isComposing: false, stopPropagation: vi.fn(), preventDefault: vi.fn() };
      return { result: picker.keydown(e), e };
    };
    for (const key of ['h', 'ArrowLeft']) {
      const { result, e } = press(key);
      expect(result).toBeNull();
      expect(e.stopPropagation).toHaveBeenCalled();
    }
    const esc = press('Escape');
    expect(esc.result).toBe('close');
    expect(esc.e.stopPropagation).toHaveBeenCalled();
    expect(esc.e.preventDefault).toHaveBeenCalled();
    const enter = press('Enter');
    expect(enter.result).toBe('submit');
    expect(enter.e.stopPropagation).toHaveBeenCalled();
    expect(enter.e.preventDefault).toHaveBeenCalled();
  });

  it('an Enter that confirms an IME composition is not a submit', () => {
    const { picker } = setup();
    const e = { key: 'Enter', isComposing: true, stopPropagation: vi.fn(), preventDefault: vi.fn() };
    expect(picker.keydown(e)).toBeNull();
    expect(e.stopPropagation).toHaveBeenCalled();
  });
});
