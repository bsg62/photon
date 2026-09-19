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
        deps.onerror(e);
      }
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
      unsubscribe?.();
      unsubscribe = undefined;
    },
  };
}

export type Theme = ReturnType<typeof createTheme>;
