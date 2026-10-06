import { describe, expect, it } from 'vitest';
import { clampSidebarWidth, GROUPS_DEFAULT, SIDEBAR_DEFAULT, SIDEBAR_GROUPS_KEY, SIDEBAR_MIN, SIDEBAR_WIDTH_KEY, storedOpenGroups, storedSidebarWidth, storeOpenGroups, storeSidebarWidth } from './sidebar';

describe('clampSidebarWidth', () => {
  it('keeps a width inside the bounds', () => {
    expect(clampSidebarWidth(300, 1200)).toBe(300);
  });

  it('stops at half the window', () => {
    expect(clampSidebarWidth(900, 1200)).toBe(600);
  });

  it('stops at the minimum', () => {
    expect(clampSidebarWidth(20, 1200)).toBe(SIDEBAR_MIN);
  });

  it('prefers the minimum when the window is too narrow for both', () => {
    expect(clampSidebarWidth(250, 200)).toBe(SIDEBAR_MIN);
  });
});

/** A `Storage` of the two calls the sidebar makes, over a plain map. */
function fakeStore(initial: Record<string, string> = {}) {
  const held = new Map(Object.entries(initial));
  return {
    held,
    getItem: (key: string) => held.get(key) ?? null,
    setItem: (key: string, value: string) => void held.set(key, value),
  };
}

/** A store that refuses everything, as a webview with storage switched off does. */
const refusing = {
  getItem: (): string | null => {
    throw new Error('SecurityError');
  },
  setItem: () => {
    throw new Error('QuotaExceededError');
  },
};

describe('the remembered sidebar width', () => {
  it('is the default until one is stored', () => {
    expect(storedSidebarWidth(fakeStore(), 1280)).toBe(SIDEBAR_DEFAULT);
    expect(storedSidebarWidth(null, 1280)).toBe(SIDEBAR_DEFAULT);
  });

  it('comes back as it was stored', () => {
    const store = fakeStore();
    storeSidebarWidth(store, 312);
    expect(store.held.get(SIDEBAR_WIDTH_KEY)).toBe('312');
    expect(storedSidebarWidth(store, 1280)).toBe(312);
  });

  // Stored on a wide window and read on a narrow one: the same clamp a drag gets.
  it('is clamped to the window it is read in', () => {
    const store = fakeStore({ [SIDEBAR_WIDTH_KEY]: '600' });
    expect(storedSidebarWidth(store, 800)).toBe(400);
    expect(storedSidebarWidth(fakeStore({ [SIDEBAR_WIDTH_KEY]: '12' }), 1280)).toBe(SIDEBAR_MIN);
  });

  it('falls back to the default for anything that is not a width', () => {
    for (const junk of ['', 'wide', 'NaN', 'Infinity', '{}', 'null']) {
      expect(storedSidebarWidth(fakeStore({ [SIDEBAR_WIDTH_KEY]: junk }), 1280), junk).toBe(SIDEBAR_DEFAULT);
    }
  });

  it('survives a store that refuses to be read or written', () => {
    expect(storedSidebarWidth(refusing, 1280)).toBe(SIDEBAR_DEFAULT);
    expect(() => storeSidebarWidth(refusing, 300)).not.toThrow();
    expect(() => storeSidebarWidth(null, 300)).not.toThrow();
  });
});

describe('the remembered open groups', () => {
  it('start as they always have: albums and searches open, people and tags closed', () => {
    expect(storedOpenGroups(fakeStore())).toEqual({ albums: true, searches: true, people: false, tags: false });
    expect(storedOpenGroups(null)).toEqual(GROUPS_DEFAULT);
  });

  it('come back as they were stored', () => {
    const store = fakeStore();
    storeOpenGroups(store, { albums: false, searches: true, people: true, tags: true });
    expect(storedOpenGroups(store)).toEqual({ albums: false, searches: true, people: true, tags: true });
  });

  // A group added by a later photon is not in what an earlier one stored, and one this
  // photon does not know is not its to draw.
  it('take a default for a group not stored, and drop one not known', () => {
    const store = fakeStore({ [SIDEBAR_GROUPS_KEY]: JSON.stringify({ people: true, places: true }) });
    expect(storedOpenGroups(store)).toEqual({ albums: true, searches: true, people: true, tags: false });
  });

  it('ignore a value that is not a yes or a no', () => {
    const store = fakeStore({ [SIDEBAR_GROUPS_KEY]: JSON.stringify({ albums: 'no', tags: 1, people: null }) });
    expect(storedOpenGroups(store)).toEqual(GROUPS_DEFAULT);
  });

  it('fall back to the defaults for anything that is not a set of groups', () => {
    for (const junk of ['', '{', 'null', '[true]', '7', '"albums"']) {
      expect(storedOpenGroups(fakeStore({ [SIDEBAR_GROUPS_KEY]: junk })), junk).toEqual(GROUPS_DEFAULT);
    }
  });

  it('hand out a set of their own each time, not the defaults themselves', () => {
    const first = storedOpenGroups(null);
    first.tags = true;
    expect(storedOpenGroups(null).tags).toBe(false);
  });

  it('survive a store that refuses to be read or written', () => {
    expect(storedOpenGroups(refusing)).toEqual(GROUPS_DEFAULT);
    expect(() => storeOpenGroups(refusing, GROUPS_DEFAULT)).not.toThrow();
  });
});
