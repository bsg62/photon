import type { ThemeChoice } from './api';

export type ResolvedTheme = 'light' | 'dark';

export interface ThemeDeps {
  load(): Promise<ThemeChoice>;
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
  let disposed = false;

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
      // A pinned theme is the user's answer to a desktop that reports the wrong scheme
      // (WebKitGTK does), so a change on the desktop must not undo it.
      unsubscribe = deps.media.onchange(() => {
        if (choice === 'system') refresh();
      });
      try {
        choice = await deps.load();
      } catch (e) {
        // A disposed instance has no caller left to show the error to, and no state left
        // that reporting it would explain, so it is dropped along with the refresh below.
        if (!disposed) deps.onerror(e);
      }
      if (disposed) return;
      // Unconditional, not `if (choice === 'system')`: a media change that arrives while
      // `load()` is still pending passes that guard and fires a transient `apply()` for the
      // still-default 'system' choice, and this call is what overwrites it with the loaded
      // choice's real result. Guarding it too would leave that transient apply as the last
      // word whenever the stored choice turns out not to be 'system'.
      refresh();
    },

    /** Applies first and saves second, so the click is answered at once. A failed save
     *  keeps the theme for this session: reverting it would punish the user for a disk
     *  error with a flash. */
    async set(next: ThemeChoice) {
      choice = next;
      refresh();
      try {
        await deps.save(next);
      } catch (e) {
        deps.onerror(e);
      }
    },

    dispose() {
      disposed = true;
      unsubscribe?.();
      unsubscribe = undefined;
    },
  };
}

export type Theme = ReturnType<typeof createTheme>;
