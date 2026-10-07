import { ownsSelectAll, type SelectAllTarget } from './nav';

/** Where a group of keys works. */
export type ShortcutGroupId = 'everywhere' | 'grid' | 'compare' | 'viewer' | 'crop' | 'slideshow' | 'video';

export interface Shortcut {
  /** The chords that do it, any one of them, each a list of keys pressed together. `Mod`
   *  is Ctrl, or ⌘ on a Mac ([`chordLabel`]); `click` and `wheel` are the mouse, for the
   *  gestures that need a key held, and `arrow` is any of the four arrow keys. */
  keys: string[][];
  does: string;
}

export interface ShortcutGroup {
  id: ShortcutGroupId;
  title: string;
  rows: Shortcut[];
}

/** Every key photon answers, by where it works: what the `?` sheet and Settings →
 *  Shortcuts both draw.
 *
 *  Written by hand, and held to the handlers by `shortcuts.test.ts`, which reads their
 *  source: a key a handler answers that is not here fails it, and so does a key listed here
 *  that no handler answers. What it cannot check is the wording - a key whose meaning
 *  changes needs its row changed with it. Pure mouse gestures (the wheel, a drag, a
 *  double-click) are not keys and are not listed. */
export const SHORTCUTS: ShortcutGroup[] = [
  {
    id: 'everywhere',
    title: 'Everywhere',
    rows: [
      { keys: [['?']], does: 'Show this list' },
      { keys: [['F11']], does: 'Fullscreen on or off' },
      { keys: [['Mod', 'F'], ['/']], does: 'Go to the search box, from the grid or the People page' },
      { keys: [['Enter']], does: 'In the search box: go to the photos' },
      { keys: [['Esc']], does: 'In the search box: clear the search, or leave an empty box' },
      // One line, where the search box's row says "from the grid or the People page" of the
      // same rule: a second line here is the one that pushes the sheet past a small window.
      { keys: [['Mod', 'B']], does: 'Hide or show the sidebar' },
      { keys: [['←'], ['→']], does: "On the sidebar's edge: resize the sidebar" },
    ],
  },
  {
    id: 'grid',
    title: 'Grid',
    rows: [
      { keys: [['←'], ['→'], ['↑'], ['↓']], does: 'Move to the next photo that way' },
      { keys: [['PgUp'], ['PgDn']], does: 'Move a screenful up or down' },
      { keys: [['Home'], ['End']], does: 'First or last photo' },
      { keys: [['Enter']], does: 'Open the photo in the viewer' },
      { keys: [['Mod', 'click']], does: 'Add a photo to the selection, or take it out' },
      { keys: [['Shift', 'click']], does: 'Select every photo up to that one' },
      // One row for every key that moves, or the list is taller than a small window: Shift
      // does the same with each of them.
      { keys: [['Shift', 'arrow']], does: 'Select up to the next photo that way; also with Home, End, PgUp and PgDn' },
      { keys: [['Mod', 'A']], does: 'Select every photo' },
      { keys: [['Esc']], does: 'Clear the selection' },
      { keys: [['C']], does: 'Compare the two to four selected photos' },
      { keys: [['.']], does: 'Star the selection, or take its stars off' },
      { keys: [['H']], does: 'Hide the selection; in Hidden, unhide it' },
      { keys: [['Mod', 'C']], does: 'Copy the selected photo as a picture' },
      { keys: [['Mod', 'Shift', 'R']], does: 'Reveal the photo in the file manager' },
    ],
  },
  {
    // After the grid, which it opens from - and where the sheet's two columns come out
    // nearest the same height, which is what lets all of it fit a small window.
    id: 'compare',
    title: 'Compare',
    rows: [
      // The range of panes: `shortcuts.test.ts` builds this row's key from `MAX_PANES`, so a
      // comparison that grows a fifth pane fails there until this says so.
      { keys: [['1–4']], does: 'Focus that photo' },
      { keys: [['Tab'], ['Shift', 'Tab']], does: 'Focus the next or the previous photo' },
      { keys: [['S'], ['.']], does: 'Star the focused photo, or take its star off' },
      { keys: [['Enter']], does: 'Open the focused photo in the viewer' },
      { keys: [['Esc']], does: 'Back to the grid' },
    ],
  },
  {
    id: 'viewer',
    title: 'Viewer',
    rows: [
      { keys: [['←'], ['→']], does: 'Previous or next photo' },
      { keys: [['Home'], ['End']], does: 'First or last photo' },
      { keys: [['Esc'], ['Backspace']], does: 'Back to the grid' },
      { keys: [['.']], does: 'Star the photo, or take its star off' },
      { keys: [['+'], ['=']], does: 'Zoom in' },
      { keys: [['-']], does: 'Zoom out' },
      { keys: [['0']], does: 'Fit the photo to the window' },
      { keys: [['Mod', 'wheel']], does: 'Zoom in or out where the pointer is' },
      { keys: [['I']], does: 'Photo information on or off' },
      { keys: [['R']], does: 'Turn the photo right' },
      { keys: [['Shift', 'R']], does: 'Turn the photo left' },
      { keys: [['C']], does: 'Crop' },
      { keys: [['H']], does: 'Hide the photo; again to bring it back' },
      { keys: [['S']], does: 'Start a slideshow from this photo' },
      { keys: [['Mod', 'C']], does: 'Copy the photo as a picture' },
    ],
  },
  {
    id: 'crop',
    title: 'While cropping',
    rows: [
      { keys: [['Enter']], does: 'Apply the crop' },
      { keys: [['Esc']], does: 'Cancel' },
    ],
  },
  {
    id: 'slideshow',
    title: 'Slideshow',
    rows: [
      { keys: [['Space']], does: 'Pause or carry on' },
      { keys: [['←'], ['→']], does: 'Step back or forward' },
      { keys: [['Esc'], ['S']], does: 'End the slideshow' },
    ],
  },
  {
    id: 'video',
    title: 'Video',
    rows: [
      { keys: [['Space']], does: 'Play or pause' },
      { keys: [['Shift', '←'], ['Shift', '→']], does: 'Back or forward five seconds' },
      { keys: [['L']], does: 'Loop on or off' },
      { keys: [['M']], does: 'Sound on or off' },
    ],
  },
];

/** A chord as the platform's keyboard spells it: `Mod` is ⌘ on a Mac and Ctrl elsewhere. */
export function chordLabel(chord: string[], mac: boolean): string[] {
  return chord.map((part) => (part === 'Mod' ? (mac ? '⌘' : 'Ctrl') : part));
}

/** A chord as a menu shows it beside the item it does: `Ctrl+Shift+R`, or `⌘+Shift+R`. */
export function keyHint(chord: string[], mac: boolean): string {
  return chordLabel(chord, mac).join('+');
}

/** Whether a keydown asks for the search box: Ctrl+F (⌘F on a Mac) from anywhere, a text
 *  entry included, and a plain `/` anywhere but in one, where it is the character. Shift is
 *  refused beside Ctrl, since that chord is not this one; beside `/` it is how some layouts
 *  type the character at all.
 *
 *  On a Mac it is ⌘F alone. Ctrl+F there is the text system's own "forward a character",
 *  in every text field, and a name being typed must not jump to the search box for it. */
export function focusesSearch(
  e: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'altKey' | 'shiftKey'>,
  target: SelectAllTarget | null,
  mac: boolean,
): boolean {
  if (e.altKey) return false;
  if (e.ctrlKey || e.metaKey) {
    if (mac && !e.metaKey) return false;
    return !e.shiftKey && e.key.toLowerCase() === 'f';
  }
  return e.key === '/' && ownsSelectAll(target);
}

/** Whether a keydown asks for the sidebar to be hidden or shown: Ctrl+B (⌘B on a Mac), from
 *  anywhere, a text entry included - the chord means nothing in a plain text field. Shift
 *  and Alt make it another chord. On a Mac it is ⌘B alone, for `focusesSearch`'s reason:
 *  Ctrl+B there is the text system's own "back a character". */
export function togglesSidebar(
  e: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'altKey' | 'shiftKey'>,
  mac: boolean,
): boolean {
  if (e.altKey || e.shiftKey) return false;
  if (!(mac ? e.metaKey : e.ctrlKey || e.metaKey)) return false;
  return e.key.toLowerCase() === 'b';
}

/** Whether a keydown asks for the shortcut sheet: a plain `?`, anywhere but in a text entry,
 *  where it is the character. A chord is left alone, as every letter key's handler leaves
 *  it: a modifier means the key belongs to the webview or the OS. Shift is not one here -
 *  it is how most layouts type `?` at all. */
export function opensShortcuts(
  e: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'altKey'>,
  target: SelectAllTarget | null,
): boolean {
  return e.key === '?' && !e.ctrlKey && !e.metaKey && !e.altKey && ownsSelectAll(target);
}
