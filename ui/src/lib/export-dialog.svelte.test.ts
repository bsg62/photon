import { describe, expect, it, vi } from 'vitest';
import { createExportDialog } from './export-dialog.svelte';

function setup(report = { written: 3, failed: 0, reason: null as string | null }) {
  const run = vi.fn(async (_ids: number[], _dest: string, _applyEdits: boolean) => report);
  const pick = vi.fn(async () => '/home/ada/Desktop' as string | null);
  const remember = vi.fn(async (_apply: boolean) => {});
  const dialog = createExportDialog({ run, pick, remember });
  return { dialog, run, pick, remember };
}

describe('createExportDialog', () => {
  it('opens with the remembered choice and the count it will write', () => {
    const { dialog } = setup();
    dialog.show([1, 2, 3], false);
    expect(dialog.visible).toBe(true);
    expect(dialog.count).toBe(3);
    expect(dialog.applyEdits).toBe(false);
  });

  it('writes nothing until a destination has been chosen', async () => {
    const { dialog, run } = setup();
    dialog.show([1, 2, 3], true);
    expect(dialog.dest).toBe(null);
    expect(await dialog.submit()).toBe(null);
    expect(run).not.toHaveBeenCalled();
    expect(dialog.visible).toBe(true);
  });

  it('remembers the checkbox as soon as it changes, not when the export runs', async () => {
    const { dialog, remember } = setup();
    dialog.show([1], true);
    await dialog.setApplyEdits(false);
    expect(remember).toHaveBeenCalledWith(false);
    expect(dialog.applyEdits).toBe(false);
  });

  it('exports what it was opened with, to the folder that was picked', async () => {
    const { dialog, run, pick } = setup();
    dialog.show([4, 5], true);
    await dialog.choose();
    expect(pick).toHaveBeenCalled();
    expect(dialog.dest).toBe('/home/ada/Desktop');
    await dialog.submit();
    expect(run).toHaveBeenCalledWith([4, 5], '/home/ada/Desktop', true);
  });

  it('keeps the dialog open when the picker is dismissed', async () => {
    const { dialog, pick } = setup();
    pick.mockResolvedValueOnce(null);
    dialog.show([1], true);
    await dialog.choose();
    expect(dialog.dest).toBe(null);
    expect(dialog.visible).toBe(true);
  });

  /** The export can take minutes; a second Enter must not start a second one. */
  it('ignores a second submit while the first is still running', async () => {
    let land!: (r: { written: number; failed: number; reason: string | null }) => void;
    const run = vi.fn(() => new Promise<{ written: number; failed: number; reason: string | null }>((r) => (land = r)));
    const dialog = createExportDialog({
      run,
      pick: async () => '/dest',
      remember: async () => {},
    });
    dialog.show([1, 2], true);
    await dialog.choose();

    const first = dialog.submit();
    expect(dialog.busy).toBe(true);
    expect(await dialog.submit()).toBe(null);

    land({ written: 2, failed: 0, reason: null });
    await first;
    expect(run).toHaveBeenCalledTimes(1);
    expect(dialog.busy).toBe(false);
  });

  describe('the message it reports', () => {
    it('counts one photo singly and names the folder', async () => {
      const { dialog } = setup({ written: 1, failed: 0, reason: null });
      dialog.show([1], true);
      await dialog.choose();
      expect(await dialog.submit()).toBe('Exported 1 photo to /home/ada/Desktop');
    });

    it('says how many of the selection landed, and why, when some did not', async () => {
      const { dialog } = setup({ written: 10, failed: 2, reason: 'item 4 not found' });
      dialog.show([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12], true);
      await dialog.choose();
      expect(await dialog.submit()).toBe(
        'Exported 10 of 12 photos to /home/ada/Desktop — item 4 not found',
      );
    });

    /** Nothing written is a failure, and the reason is the whole of what the user gets. */
    it('throws rather than reports when nothing could be written', async () => {
      const { dialog } = setup({ written: 0, failed: 2, reason: 'permission denied' });
      dialog.show([1, 2], true);
      await dialog.choose();
      await expect(dialog.submit()).rejects.toThrow('permission denied');
      expect(dialog.visible).toBe(false);
    });
  });
});
