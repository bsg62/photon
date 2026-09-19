import { describe, expect, it, vi } from 'vitest';
import type { ThemeChoice } from './api';
import { createTheme, resolveTheme } from './theme.svelte';

function setup(opts: { stored?: ThemeChoice; osDark?: boolean; load?: () => Promise<ThemeChoice> } = {}) {
  let osDark = opts.osDark ?? false;
  const listeners = new Set<() => void>();
  const apply = vi.fn();
  const save = vi.fn(async (_choice: ThemeChoice) => {});
  const onerror = vi.fn();
  const theme = createTheme({
    load: opts.load ?? (async () => opts.stored ?? 'system'),
    save,
    media: {
      dark: () => osDark,
      onchange: (cb) => {
        listeners.add(cb);
        return () => listeners.delete(cb);
      },
    },
    apply,
    onerror,
  });
  const setOs = (dark: boolean) => {
    osDark = dark;
    for (const cb of [...listeners]) cb();
  };
  return { theme, apply, save, onerror, setOs, listeners };
}

describe('resolveTheme', () => {
  it('follows the desktop only when asked to', () => {
    expect(resolveTheme('system', true)).toBe('dark');
    expect(resolveTheme('system', false)).toBe('light');
    expect(resolveTheme('light', true)).toBe('light');
    expect(resolveTheme('dark', false)).toBe('dark');
  });
});

describe('createTheme', () => {
  it('applies the stored choice on init', async () => {
    const { theme, apply } = setup({ stored: 'dark', osDark: false });
    await theme.init();
    expect(theme.choice).toBe('dark');
    expect(theme.resolved).toBe('dark');
    expect(apply).toHaveBeenLastCalledWith('dark', 'dark');
  });

  it('follows the desktop while the choice is system', async () => {
    const { theme, apply, setOs } = setup({ stored: 'system', osDark: false });
    await theme.init();
    expect(apply).toHaveBeenLastCalledWith('light', 'system');
    setOs(true);
    expect(theme.resolved).toBe('dark');
    expect(apply).toHaveBeenLastCalledWith('dark', 'system');
  });

  it('ignores the desktop once the user has pinned a theme', async () => {
    const { theme, apply, setOs } = setup({ stored: 'light', osDark: false });
    await theme.init();
    apply.mockClear();
    setOs(true);
    expect(theme.resolved).toBe('light');
    expect(apply).not.toHaveBeenCalled();
  });

  it('applies a new choice before saving it', async () => {
    const { theme, apply, save } = setup({ stored: 'system', osDark: false });
    await theme.init();
    let appliedWhenSaved: unknown;
    save.mockImplementationOnce(async () => {
      appliedWhenSaved = apply.mock.lastCall;
    });
    await theme.set('dark');
    expect(appliedWhenSaved).toEqual(['dark', 'dark']);
    expect(save).toHaveBeenCalledWith('dark');
    expect(theme.choice).toBe('dark');
  });

  it('keeps the applied theme and reports when the save fails', async () => {
    const { theme, apply, save, onerror } = setup({ stored: 'system', osDark: false });
    await theme.init();
    const failure = new Error('disk full');
    save.mockRejectedValueOnce(failure);
    await theme.set('dark');
    expect(theme.resolved).toBe('dark');
    expect(apply).toHaveBeenLastCalledWith('dark', 'dark');
    expect(onerror).toHaveBeenCalledWith(failure);
  });

  it('falls back to the desktop and reports when the stored choice cannot be read', async () => {
    const failure = new Error('locked');
    const { theme, apply, onerror } = setup({ osDark: true, load: async () => Promise.reject(failure) });
    await theme.init();
    expect(theme.choice).toBe('system');
    expect(apply).toHaveBeenLastCalledWith('dark', 'system');
    expect(onerror).toHaveBeenCalledWith(failure);
  });

  it('stops listening to the desktop on dispose', async () => {
    const { theme, apply, setOs, listeners } = setup({ stored: 'system', osDark: false });
    await theme.init();
    theme.dispose();
    expect(listeners.size).toBe(0);
    apply.mockClear();
    setOs(true);
    expect(apply).not.toHaveBeenCalled();
  });
});
