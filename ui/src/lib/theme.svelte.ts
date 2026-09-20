import type { ThemeChoice } from './api';

export type ResolvedTheme = 'light' | 'dark';

export interface ThemeDeps {
  load(): Promise<ThemeChoice>;
  /** The launch mirror's value, or null when there is none to read. */
  mirrored(): ThemeChoice | null;
  save(choice: ThemeChoice): Promise<void>;
  /** The desktop's colour scheme. `onchange` returns its own unsubscribe. */
  media: { dark(): boolean; onchange(cb: () => void): () => void };
  /** The side effects: the DOM attribute, the launch mirror, the title bar. */
  apply(resolved: ResolvedTheme, choice: ThemeChoice): void;
  onerror(e: unknown): void;
}

export function resolveTheme(choice: ThemeChoice, osDark: boolean): ResolvedTheme {
  if (choice === 'system') return osDark ? 'dark' : 'light';
  return choice;
}

/** The colour scheme: what the user chose, and what that comes to on this desktop right
 *  now. Everything that touches the DOM, storage or Tauri is injected, so this runs under
 *  vitest's node environment. */
export function createTheme(deps: ThemeDeps) {
  let choice = $state<ThemeChoice>('system');
  let resolved = $state<ResolvedTheme>(resolveTheme('system', deps.media.dark()));
  let unsubscribe: (() => void) | undefined;
  /** Bumped by dispose() and by every init(), so a load or media callback started under an
   *  earlier generation can tell it is no longer current. In production `theme` is a module
   *  singleton that App.svelte disposes on unmount and re-inits on remount (HMR), the same
   *  lifecycle library.svelte.ts documents for `LibraryStore` and solves the same way — a
   *  one-way `disposed` boolean would never let the singleton come back to life. */
  let generation = 0;
  /** True once `set()` has been called since the current init's load started, so the load's
   *  eventual answer does not clobber a choice the user made while it was still pending. */
  let setDuringLoad = false;

  function refresh() {
    resolved = resolveTheme(choice, deps.media.dark());
    deps.apply(resolved, choice);
  }

  return {
    get choice() {
      return choice;
    },
    get resolved() {
      return resolved;
    },

    async init() {
      // Tear down a previous subscription first: calling init() again without an
      // intervening dispose() (a stray double-mount) must not leak the old listener.
      unsubscribe?.();
      const myGeneration = ++generation;
      setDuringLoad = false;
      // A pinned theme is the user's answer to a desktop that reports the wrong scheme
      // (WebKitGTK does), so a change on the desktop must not undo it.
      unsubscribe = deps.media.onchange(() => {
        if (choice === 'system') refresh();
      });
      try {
        const loaded = await deps.load();
        // A set() that happened while this load was in flight wins: the load's answer is
        // stale by the time it arrives, and applying it now would show the old choice while
        // the database already holds the new one.
        if (myGeneration === generation && !setDuringLoad) choice = loaded;
      } catch (e) {
        // A disposed (or superseded) instance has no caller left to show the error to, and
        // no state left that reporting it would explain, so it is dropped along with the
        // refresh below.
        if (myGeneration === generation) {
          // Fall back to the launch mirror rather than to the default `system`. The mirror
          // is the last choice known to have been saved, and what theme-boot.js already
          // painted the first frame with, so adopting it keeps the screen as it is. The
          // default would not merely be wrong on screen: `refresh()` below calls `apply`,
          // which writes the choice back into the mirror, so one unreadable load would
          // overwrite a pinned choice and cost the no-flash boot on every later launch.
          // Not applied when `set()` won the race, for the same reason the load's own
          // answer is not: the user's newer choice stands.
          if (!setDuringLoad) choice = deps.mirrored() ?? choice;
          deps.onerror(e);
        }
      }
      if (myGeneration !== generation) return;
      // Unconditional, not `if (choice === 'system')`: a media change that arrives while
      // `load()` is still pending passes that guard and fires a transient `apply()` for the
      // still-default 'system' choice, and this call is what overwrites it with the loaded
      // choice's real result (or, when `set()` won the race above, with the user's pinned
      // one). Guarding it too would leave that transient apply as the last word whenever the
      // final choice turns out not to be 'system'.
      refresh();
    },

    /** Applies first and saves second, so the click is answered at once. A failed save
     *  keeps the theme for this session: reverting it would punish the user for a disk
     *  error with a flash. */
    async set(next: ThemeChoice) {
      choice = next;
      setDuringLoad = true;
      refresh();
      try {
        await deps.save(next);
      } catch (e) {
        deps.onerror(e);
      }
    },

    dispose() {
      generation++;
      unsubscribe?.();
      unsubscribe = undefined;
    },
  };
}

export type Theme = ReturnType<typeof createTheme>;
