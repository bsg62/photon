import { describe, expect, it } from 'vitest';
import { MAX_PANES } from './compare.svelte';
import { chordLabel, focusesSearch, keyHint, opensShortcuts, SHORTCUTS, type ShortcutGroupId } from './shortcuts';

/** The files that answer keys, as text. */
const sources = import.meta.glob(['../App.svelte', '../components/*.svelte', './*.ts', '!./*.test.ts'], {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

/** Which groups of the list a file's keys are listed under. A key the file answers must be
 *  in one of them, and a key a group lists must be answered by a file that names the group.
 *  A handler in a file that is not here is not checked: add the file with its group. */
const ANSWERED_IN: Record<string, ShortcutGroupId[]> = {
  '../App.svelte': ['everywhere', 'grid'],
  '../components/SearchBar.svelte': ['everywhere'],
  './shortcuts.ts': ['everywhere'],
  '../components/Grid.svelte': ['grid'],
  './copy-photo.ts': ['grid', 'viewer'],
  '../components/Viewer.svelte': ['viewer', 'crop', 'slideshow'],
  './video-player.svelte.ts': ['video'],
  '../components/Compare.svelte': ['compare'],
};

/** Parts of a chord that are not a key a handler compares `e.key` with. */
const MODIFIERS = ['Mod', 'Shift', 'click', 'wheel'];

const NAMES: Record<string, string> = {
  ' ': 'Space',
  Escape: 'Esc',
  ArrowLeft: '←',
  ArrowRight: '→',
  ArrowUp: '↑',
  ArrowDown: '↓',
};

/** A key as the list spells it: a letter in capitals, a digit as the range of panes. */
function listed(key: string): string {
  if (/^[1-9]$/.test(key)) return `1–${MAX_PANES}`;
  return NAMES[key] ?? (key.length === 1 ? key.toUpperCase() : key);
}

/** Every key a file's handlers compare against: `e.key === 'x'` (and `!==`, `>=`, with or
 *  without `toLowerCase()`), a `case 'x':`, and a list of keys tested with `includes`. */
function keysIn(source: string): Set<string> {
  const code = source
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/^\s*\/\/.*$/gm, '');
  const keys = new Set<string>();
  // `e.key`, or a local named `key` holding it - never another object's `.key`: a sort has
  // one too (`sort.key === 'date'`).
  for (const m of code.matchAll(/(?<![.\w])(?:e\.)?key(?:\.toLowerCase\(\))? (?:===|!==|>=) '([^']+)'/g)) {
    keys.add(listed(m[1]));
  }
  for (const m of code.matchAll(/\bcase '([^']+)':/g)) keys.add(listed(m[1]));
  for (const m of code.matchAll(/\[((?:\s*'[^']+',?)+\s*)\](?:\.includes\(e\.key\)|;)/g)) {
    for (const k of m[1].matchAll(/'([^']+)'/g)) keys.add(listed(k[1]));
  }
  return keys;
}

const group = (id: ShortcutGroupId) => SHORTCUTS.find((g) => g.id === id)!;
const keysOf = (id: ShortcutGroupId) =>
  new Set(
    group(id)
      .rows.flatMap((row) => row.keys.flat())
      .filter((k) => !MODIFIERS.includes(k)),
  );

describe('the shortcut list', () => {
  it('reads keys out of every file it is checked against', () => {
    // The check below passes on a file it could not read a key from. Each of these files
    // answers at least one, so an empty set means the pattern no longer fits the code.
    for (const file of Object.keys(ANSWERED_IN)) {
      expect(sources[file], file).toBeDefined();
      expect(keysIn(sources[file]).size, file).toBeGreaterThan(0);
    }
    expect([...keysIn(sources['../components/Viewer.svelte'])].sort()).toEqual(
      ['+', '-', '.', '0', '=', 'Backspace', 'C', 'End', 'Enter', 'Esc', 'H', 'Home', 'I', 'R', 'S', 'Space', '←', '→'].sort(),
    );
    expect([...keysIn(sources['../components/Grid.svelte'])].sort()).toEqual(
      ['.', 'A', 'C', 'End', 'Enter', 'Esc', 'H', 'Home', 'R', '←', '→', '↑', '↓'].sort(),
    );
    expect([...keysIn(sources['./video-player.svelte.ts'])].sort()).toEqual(['L', 'M', 'Space', '←', '→'].sort());
    expect([...keysIn(sources['../components/Compare.svelte'])].sort()).toEqual(
      ['.', `1–${MAX_PANES}`, 'Enter', 'Esc', 'S', 'Tab'].sort(),
    );
  });

  it.each(Object.entries(ANSWERED_IN))('names every key %s answers', (file, groups) => {
    const known = new Set(groups.flatMap((id) => [...keysOf(id)]));
    const unlisted = [...keysIn(sources[file])].filter((key) => !known.has(key));
    expect(unlisted).toEqual([]);
  });

  it.each(SHORTCUTS.map((g) => g.id))('lists no key under %s that nothing answers', (id) => {
    const answered = new Set(
      Object.entries(ANSWERED_IN)
        .filter(([, groups]) => groups.includes(id))
        .flatMap(([file]) => [...keysIn(sources[file])]),
    );
    expect([...keysOf(id)].filter((key) => !answered.has(key))).toEqual([]);
  });

  it('has a row for every group, and says what each key does', () => {
    expect(SHORTCUTS.map((g) => g.id)).toEqual(['everywhere', 'grid', 'compare', 'viewer', 'crop', 'slideshow', 'video']);
    for (const g of SHORTCUTS) {
      expect(g.title, g.id).not.toBe('');
      expect(g.rows.length, g.id).toBeGreaterThan(0);
      for (const row of g.rows) {
        expect(row.does, g.id).not.toBe('');
        expect(row.keys.length, row.does).toBeGreaterThan(0);
        for (const chord of row.keys) expect(chord.length, row.does).toBeGreaterThan(0);
      }
    }
  });

  it('is the list the key that opens it is in', () => {
    expect(group('everywhere').rows.some((row) => row.keys.some((chord) => chord.join('+') === '?'))).toBe(true);
  });
});

describe('chordLabel', () => {
  it('spells Mod as the platform does', () => {
    expect(chordLabel(['Mod', 'Shift', 'R'], false)).toEqual(['Ctrl', 'Shift', 'R']);
    expect(chordLabel(['Mod', 'C'], true)).toEqual(['⌘', 'C']);
    expect(chordLabel(['Esc'], true)).toEqual(['Esc']);
  });
});

describe('keyHint', () => {
  it('is the chord on one line, spelled as the platform does', () => {
    expect(keyHint(['Mod', 'Shift', 'R'], false)).toBe('Ctrl+Shift+R');
    expect(keyHint(['Mod', 'C'], true)).toBe('⌘+C');
    expect(keyHint(['H'], false)).toBe('H');
  });
});

describe('focusesSearch', () => {
  const key = (over: Partial<Parameters<typeof focusesSearch>[0]> = {}) => ({
    key: 'f',
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    shiftKey: false,
    ...over,
  });

  it('answers Ctrl+F, wherever the focus is', () => {
    expect(focusesSearch(key({ ctrlKey: true }), null, false)).toBe(true);
    expect(focusesSearch(key({ ctrlKey: true }), { tagName: 'DIV' }, false)).toBe(true);
    // Caps Lock, or a layout that reports the capital.
    expect(focusesSearch(key({ key: 'F', ctrlKey: true }), null, false)).toBe(true);
    // From a text entry too: the chord is not a character there.
    expect(focusesSearch(key({ ctrlKey: true }), { tagName: 'INPUT' }, false)).toBe(true);
  });

  it('is Cmd+F on a Mac, and never Ctrl+F there', () => {
    expect(focusesSearch(key({ metaKey: true }), null, true)).toBe(true);
    expect(focusesSearch(key({ metaKey: true }), { tagName: 'INPUT' }, true)).toBe(true);
    // The text system's "forward a character": a name field keeps it.
    expect(focusesSearch(key({ ctrlKey: true }), { tagName: 'INPUT' }, true)).toBe(false);
    expect(focusesSearch(key({ ctrlKey: true }), null, true)).toBe(false);
  });

  it('leaves a plain f, and the chords that are not this one, alone', () => {
    expect(focusesSearch(key(), null, false)).toBe(false);
    expect(focusesSearch(key({ ctrlKey: true, shiftKey: true }), null, false)).toBe(false);
    expect(focusesSearch(key({ ctrlKey: true, altKey: true }), null, false)).toBe(false);
    expect(focusesSearch(key({ key: 'g', ctrlKey: true }), null, false)).toBe(false);
    expect(focusesSearch(key({ metaKey: true, shiftKey: true }), null, true)).toBe(false);
  });

  it('answers a plain slash, which some layouts type with Shift', () => {
    expect(focusesSearch(key({ key: '/' }), null, false)).toBe(true);
    expect(focusesSearch(key({ key: '/' }), null, true)).toBe(true);
    expect(focusesSearch(key({ key: '/', shiftKey: true }), { tagName: 'DIV' }, false)).toBe(true);
    expect(focusesSearch(key({ key: '/', altKey: true }), null, false)).toBe(false);
    expect(focusesSearch(key({ key: '/', ctrlKey: true }), null, false)).toBe(false);
  });

  it('is a character in a text entry', () => {
    // A path typed into a name field, a date into the search box itself.
    expect(focusesSearch(key({ key: '/' }), { tagName: 'INPUT' }, false)).toBe(false);
    expect(focusesSearch(key({ key: '/' }), { tagName: 'textarea' }, false)).toBe(false);
    expect(focusesSearch(key({ key: '/' }), { tagName: 'DIV', isContentEditable: true }, false)).toBe(false);
  });
});

describe('opensShortcuts', () => {
  const key = (over: Partial<Parameters<typeof opensShortcuts>[0]> = {}) => ({
    key: '?',
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    ...over,
  });

  it('answers a plain question mark', () => {
    expect(opensShortcuts(key(), null)).toBe(true);
    expect(opensShortcuts(key(), { tagName: 'DIV' })).toBe(true);
    expect(opensShortcuts(key({ key: '/' }), null)).toBe(false);
  });

  it('leaves a chord to the webview and the OS', () => {
    expect(opensShortcuts(key({ ctrlKey: true }), null)).toBe(false);
    expect(opensShortcuts(key({ metaKey: true }), null)).toBe(false);
    expect(opensShortcuts(key({ altKey: true }), null)).toBe(false);
  });

  it('is a character in a text entry', () => {
    // The search box, a name field, a caption: typing "?" there types it.
    expect(opensShortcuts(key(), { tagName: 'INPUT' })).toBe(false);
    expect(opensShortcuts(key(), { tagName: 'textarea' })).toBe(false);
    expect(opensShortcuts(key(), { tagName: 'DIV', isContentEditable: true })).toBe(false);
  });
});
