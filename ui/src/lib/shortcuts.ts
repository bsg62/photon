import { ownsSelectAll, type SelectAllTarget } from './nav';

/** Where a group of keys works. */
export type ShortcutGroupId = 'everywhere' | 'grid' | 'viewer' | 'crop' | 'slideshow' | 'video' | 'compare';

export interface Shortcut {
  /** The chords that do it, any one of them, each a list of keys pressed together. `Mod`
   *  is Ctrl, or ⌘ on a Mac ([`chordLabel`]); `click` is the mouse, for the two selection
   *  gestures that need a key held. */
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
 *  changes needs its row changed with it. Pure mouse gestures (the wheel, a drag) are not
 *  keys and are not listed. */
export const SHORTCUTS: ShortcutGroup[] = [
  {
    id: 'everywhere',
    title: 'Everywhere',
    rows: [
      { keys: [['?']], does: 'Show this list' },
      { keys: [['F11']], does: 'Fullscreen on or off' },
      { keys: [['Esc']], does: 'In the search box: clear the search' },
      { keys: [['←'], ['→']], does: "On the sidebar's edge: resize the sidebar" },
    ],
  },
  {
    id: 'grid',
    title: 'Grid',
    rows: [
      { keys: [['←'], ['→'], ['↑'], ['↓']], does: 'Move to the next photo that way' },
      { keys: [['Home'], ['End']], does: 'First or last photo' },
      { keys: [['Enter']], does: 'Open the photo in the viewer' },
      { keys: [['Mod', 'click']], does: 'Add a photo to the selection, or take it out' },
      { keys: [['Shift', 'click']], does: 'Select every photo up to that one' },
      { keys: [['Mod', 'A']], does: 'Select every photo' },
      { keys: [['Esc']], does: 'Clear the selection' },
      { keys: [['C']], does: 'Compare the two to four selected photos' },
      { keys: [['H']], does: 'Hide the selection; in Hidden, unhide it' },
      { keys: [['Mod', 'C']], does: 'Copy the selected photo as a picture' },
      { keys: [['Mod', 'Shift', 'R']], does: 'Reveal the photo in the file manager' },
    ],
  },
  {
    id: 'viewer',
    title: 'Viewer',
    rows: [
      { keys: [['←'], ['→']], does: 'Previous or next photo' },
      { keys: [['Home'], ['End']], does: 'First or last photo' },
      { keys: [['Esc'], ['Backspace']], does: 'Back to the grid' },
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
  {
    id: 'compare',
    title: 'Compare',
    rows: [
      // The range of panes: `shortcuts.test.ts` builds this row's key from `MAX_PANES`, so a
      // comparison that grows a fifth pane fails there until this says so.
      { keys: [['1–4']], does: 'Focus that photo' },
      { keys: [['Tab'], ['Shift', 'Tab']], does: 'Focus the next or the previous photo' },
      { keys: [['S']], does: 'Star the focused photo, or take its star off' },
      { keys: [['Enter']], does: 'Open the focused photo in the viewer' },
      { keys: [['Esc']], does: 'Back to the grid' },
    ],
  },
];

/** A chord as the platform's keyboard spells it: `Mod` is ⌘ on a Mac and Ctrl elsewhere. */
export function chordLabel(chord: string[], mac: boolean): string[] {
  return chord.map((part) => (part === 'Mod' ? (mac ? '⌘' : 'Ctrl') : part));
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
