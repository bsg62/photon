import { beforeEach, describe, expect, it, vi } from 'vitest';

// `setWindowTheme` throws synchronously here, the way `getCurrentWindow()` does outside an
// initialized Tauri window - the case the fix guards against.
vi.mock('./api', () => ({
  api: {
    theme: () => Promise.resolve('system'),
    setTheme: () => Promise.resolve(),
    setWindowTheme: () => {
      throw new Error('no window');
    },
  },
}));

const reportError = vi.fn();
// `app-theme.svelte.ts` captures `library.reportError` by reference at module load, so the
// spy must exist before that import, not be attached to the real singleton afterwards.
vi.mock('./library.svelte', () => ({ library: { reportError } }));

describe('app-theme.svelte.ts apply()', () => {
  beforeEach(() => {
    vi.stubGlobal('document', { documentElement: { dataset: {} } });
    vi.stubGlobal('localStorage', { setItem: () => {} });
    vi.stubGlobal('window', {
      matchMedia: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
    });
    reportError.mockClear();
  });

  it('reports a synchronous throw from the title bar call instead of throwing it into init()', async () => {
    const { theme } = await import('./app-theme.svelte');
    // Before the fix, the synchronous throw inside apply() escapes theme.init()'s promise
    // chain as an unhandled rejection that never reaches reportError.
    await expect(theme.init()).resolves.toBeUndefined();
    // The throw is asynchronous once routed through Promise.resolve(); let it settle.
    await Promise.resolve();
    await Promise.resolve();
    expect(reportError).toHaveBeenCalled();
  });
});
