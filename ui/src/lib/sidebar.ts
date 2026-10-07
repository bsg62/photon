/** Sidebar width, as set by dragging the splitter to its right. */

export const SIDEBAR_DEFAULT = 260;
export const SIDEBAR_MIN = 160;
/** Keyboard step for the focused splitter. */
export const SIDEBAR_STEP = 16;

/** Clamps a requested width to [SIDEBAR_MIN, half the window]. The minimum wins when the
 *  window is too narrow for both, so the folder list never collapses to nothing. */
export function clampSidebarWidth(width: number, viewport: number): number {
  return Math.round(Math.max(SIDEBAR_MIN, Math.min(width, viewport / 2)));
}

/** Which of the sidebar's collection groups are unfolded. */
export interface OpenGroups {
  albums: boolean;
  searches: boolean;
  people: boolean;
  tags: boolean;
}

/** Albums and searches start open because they are the user's own; People and Tags start
 *  closed because a real library has hundreds of each, and the years below must stay
 *  reachable. */
export const GROUPS_DEFAULT: Readonly<OpenGroups> = { albums: true, searches: true, people: false, tags: false };

/** Where the sidebar's width and open groups are remembered: the webview's `localStorage`,
 *  beside the theme's first-paint mirror, not the library's settings table. They are how
 *  this window is laid out on this machine, not a fact about the library, and read here
 *  they are known before the first paint - from the database the sidebar would open at the
 *  default width and jump once the answer arrived. */
export const SIDEBAR_WIDTH_KEY = 'photon.sidebar.width';
export const SIDEBAR_GROUPS_KEY = 'photon.sidebar.groups';
export const SIDEBAR_HIDDEN_KEY = 'photon.sidebar.hidden';

/** The two calls made of a `Storage`. */
export type SidebarStore = Pick<Storage, 'getItem' | 'setItem'>;

/** The webview's `localStorage`, or null where there is none to be had: reading the
 *  property itself throws in a webview with storage switched off. */
export function browserStore(): SidebarStore | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

function read(store: SidebarStore | null, key: string): string | null {
  try {
    return store?.getItem(key) ?? null;
  } catch {
    return null;
  }
}

/** Nothing depends on a write but the next launch's layout, so a refused one is dropped. */
function write(store: SidebarStore | null, key: string, value: string): void {
  try {
    store?.setItem(key, value);
  } catch {
    // Storage full or switched off: this session keeps what it has.
  }
}

/** The width to open the sidebar at in a window `viewport` wide: the one last dragged to,
 *  clamped as a drag is - it may have been stored in a wider window - or the default. */
export function storedSidebarWidth(store: SidebarStore | null, viewport: number): number {
  const stored = read(store, SIDEBAR_WIDTH_KEY);
  // `Number('')` is 0, which is a number and not a width anyone chose.
  const width = stored === null || stored.trim() === '' ? Number.NaN : Number(stored);
  return clampSidebarWidth(Number.isFinite(width) ? width : SIDEBAR_DEFAULT, viewport);
}

export function storeSidebarWidth(store: SidebarStore | null, width: number): void {
  write(store, SIDEBAR_WIDTH_KEY, String(width));
}

/** The groups as they were left, each one that was not stored (or not as a yes or a no) at
 *  its default. A fresh object every time: the caller makes it its state. */
export function storedOpenGroups(store: SidebarStore | null): OpenGroups {
  const open = { ...GROUPS_DEFAULT };
  let stored: unknown = null;
  try {
    stored = JSON.parse(read(store, SIDEBAR_GROUPS_KEY) ?? 'null');
  } catch {
    return open;
  }
  if (typeof stored !== 'object' || stored === null) return open;
  for (const group of Object.keys(open) as (keyof OpenGroups)[]) {
    const value = (stored as Record<string, unknown>)[group];
    if (typeof value === 'boolean') open[group] = value;
  }
  return open;
}

export function storeOpenGroups(store: SidebarStore | null, open: OpenGroups): void {
  write(store, SIDEBAR_GROUPS_KEY, JSON.stringify(open));
}

/** Whether the sidebar was left hidden. Only a stored yes hides it: hidden, the way back is
 *  one small button and a key, so anything else leaves it where a new user expects it. */
export function storedSidebarHidden(store: SidebarStore | null): boolean {
  return read(store, SIDEBAR_HIDDEN_KEY) === 'true';
}

export function storeSidebarHidden(store: SidebarStore | null, hidden: boolean): void {
  write(store, SIDEBAR_HIDDEN_KEY, String(hidden));
}
