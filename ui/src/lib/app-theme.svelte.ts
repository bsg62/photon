import { api } from './api';
import { library } from './library.svelte';
import { createTheme } from './theme.svelte';

/** Read by `public/theme-boot.js` before the first paint. The boot script repeats this
 *  string, since a classic script cannot import; theme-boot.test.ts reads with this one. */
export const THEME_MIRROR_KEY = 'photon.theme';

const osDark = () => window.matchMedia('(prefers-color-scheme: dark)');

/** The app's theme. The logic is `createTheme`'s; this is the wiring it is injected with,
 *  in its own module so importing `createTheme` in a test touches no `window`. */
export const theme = createTheme({
  load: () => api.theme(),
  save: (choice) => api.setTheme(choice),
  media: {
    // `createTheme` asks once at creation, which is at import - and theme-boot.test.ts
    // imports this module for the key, under node, where there is no window.
    dark: () => typeof window !== 'undefined' && osDark().matches,
    onchange: (cb) => {
      const query = osDark();
      query.addEventListener('change', cb);
      return () => query.removeEventListener('change', cb);
    },
  },
  apply: (resolved, choice) => {
    document.documentElement.dataset.theme = resolved;
    try {
      localStorage.setItem(THEME_MIRROR_KEY, choice);
    } catch {
      // Only the next launch's first frame depends on it.
    }
    // null hands the title bar back to the desktop.
    api.setWindowTheme(choice === 'system' ? null : choice).catch(library.reportError);
  },
  onerror: library.reportError,
});
