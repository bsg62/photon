import type { ExportReport } from './api';

/** "1 photo" / "12 photos", for a message naming a specific count. */
function counted(n: number): string {
  return n === 1 ? '1 photo' : `${n.toLocaleString()} photos`;
}

/** The export dialog: which photos, where to, whether edits are rendered into the copies,
 *  and the one-export-at-a-time guard.
 *
 *  Everything with a decision in it lives here rather than in the component, because there
 *  is no component harness in this project. The folder picker, the backend call and the
 *  remembered checkbox are injected, so this runs under vitest's node environment.
 *
 *  The selection is captured at `show()`: the dialog takes focus, and a scan landing while
 *  it is open can rebind what the grid has selected. */
/** The sizes the dialog offers: the longest edge a copy may have, in pixels, with 0 for
 *  the photo's own size (a select's value is never null). Screens' widths, since a copy made smaller is one to look at or
 *  send. */
export const EXPORT_SIZES: { value: number; label: string }[] = [
  { value: 0, label: 'Original size' },
  { value: 3840, label: '3840 px' },
  { value: 2560, label: '2560 px' },
  { value: 1920, label: '1920 px' },
  { value: 1280, label: '1280 px' },
];

export function createExportDialog(deps: {
  run: (ids: number[], dest: string, applyEdits: boolean, maxEdge: number | null) => Promise<ExportReport>;
  /** The system folder picker; null when the user dismissed it. */
  pick: () => Promise<string | null>;
  /** Refuses a destination photon will not write into - today, one inside a watched folder.
   *  Asked here rather than at export time so the answer can be shown while the dialog is
   *  still open, with the folder the user picked still in hand. */
  check: (dest: string) => Promise<void>;
  /** Stores the checkbox, which is remembered between exports. */
  remember: (apply: boolean) => Promise<void>;
}) {
  let visible = $state(false);
  let dest = $state<string | null>(null);
  let applyEdits = $state(true);
  let maxEdge = $state<number | null>(null);
  let busy = $state(false);
  let problem = $state<string | null>(null);
  let ids = $state<number[]>([]);

  return {
    get visible(): boolean {
      return visible;
    },

    get dest(): string | null {
      return dest;
    },

    get applyEdits(): boolean {
      return applyEdits;
    },

    /** The longest edge a copy may have, or null for full size. Not remembered between
     *  exports, unlike the checkbox: a size left on from last time would quietly shrink the
     *  next export, and that one might be the archive. */
    get maxEdge(): number | null {
      return maxEdge;
    },

    setMaxEdge(next: number | null) {
      maxEdge = next !== null && next > 0 ? next : null;
    },

    get busy(): boolean {
      return busy;
    },

    /** Why the folder that was picked cannot be used, or null. */
    get problem(): string | null {
      return problem;
    },

    /** How many photos the export will write, for the dialog's own title: the selection is
     *  behind the dialog and cannot be counted by eye once it has focus. */
    get count(): number {
      return ids.length;
    },

    show(selection: number[], remembered: boolean) {
      ids = [...selection];
      applyEdits = remembered;
      maxEdge = null;
      dest = null;
      problem = null;
      busy = false;
      visible = true;
    },

    close() {
      visible = false;
    },

    /** Stores the checkbox as it is ticked rather than when the export runs: an export the
     *  user cancels still told us how they want exports to work. Put back if the store
     *  fails, so the box and the setting cannot disagree. */
    async setApplyEdits(next: boolean): Promise<void> {
      const previous = applyEdits;
      applyEdits = next;
      try {
        await deps.remember(next);
      } catch (e) {
        applyEdits = previous;
        throw e;
      }
    },

    /** Asks for a destination and checks it. A dismissed picker leaves the dialog exactly as
     *  it was - there is nothing to report and nothing to undo. A refused folder is shown
     *  against the field rather than accepted and rejected later, when the dialog would be
     *  gone and the folder forgotten. */
    async choose(): Promise<void> {
      const picked = await deps.pick();
      if (picked === null) return;
      try {
        await deps.check(picked);
        dest = picked;
        problem = null;
      } catch (e) {
        dest = null;
        problem = e instanceof Error ? e.message : String(e);
      }
    },

    /** Runs the export and returns the line to report, or null when there was nothing to
     *  do. Throws when nothing at all could be written: there the reason is the whole of
     *  what the user gets, and an error is what carries it to the same corner of the
     *  screen.
     *
     *  Closed before the call, not after: an export can take minutes, and a dialog sitting
     *  over the progress it started would hide it. `busy` is what stops a second Enter
     *  landing before the close has rendered. */
    async submit(): Promise<string | null> {
      if (busy || dest === null || ids.length === 0) return null;
      const to = dest;
      const asked = ids.length;
      busy = true;
      visible = false;
      try {
        const done = await deps.run(ids, to, applyEdits, maxEdge);
        if (done.written === 0) {
          throw new Error(done.reason ?? `Nothing could be exported to ${to}`);
        }
        const landed =
          done.failed === 0
            ? counted(done.written)
            : `${done.written.toLocaleString()} of ${counted(asked)}`;
        const why = done.failed > 0 && done.reason ? ` — ${done.reason}` : '';
        return `Exported ${landed} to ${to}${why}`;
      } finally {
        busy = false;
      }
    },
  };
}

export type ExportDialog = ReturnType<typeof createExportDialog>;
