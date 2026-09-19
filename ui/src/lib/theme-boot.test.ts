import { describe, expect, it } from 'vitest';
import boot from '../../public/theme-boot.js?raw';
import { THEME_MIRROR_KEY } from './app-theme.svelte';

/** Runs the boot script against fakes of the three globals it touches. */
function run(opts: { stored?: string | null; osDark: boolean; storageThrows?: boolean }): string | undefined {
  const dataset: Record<string, string> = {};
  const localStorage = {
    getItem: (key: string) => {
      if (opts.storageThrows) throw new Error('blocked');
      return key === THEME_MIRROR_KEY ? (opts.stored ?? null) : null;
    },
  };
  const matchMedia = (query: string) => ({ matches: query === '(prefers-color-scheme: dark)' && opts.osDark });
  new Function('localStorage', 'matchMedia', 'document', boot)(localStorage, matchMedia, {
    documentElement: { dataset },
  });
  return dataset.theme;
}

describe('theme-boot.js', () => {
  it('uses a pinned theme whatever the desktop says', () => {
    expect(run({ stored: 'light', osDark: true })).toBe('light');
    expect(run({ stored: 'dark', osDark: false })).toBe('dark');
  });

  it('follows the desktop for system, for nothing stored, and for nonsense', () => {
    for (const stored of ['system', null, 'sepia']) {
      expect(run({ stored, osDark: true })).toBe('dark');
      expect(run({ stored, osDark: false })).toBe('light');
    }
  });

  it('follows the desktop when storage cannot be read', () => {
    expect(run({ storageThrows: true, osDark: true })).toBe('dark');
  });
});
