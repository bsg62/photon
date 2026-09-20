import type { TagWrite } from './api';

export type TagPickerMode = 'add' | 'remove';

/** "1 photo" / "12 photos", for a message naming a specific count. */
function counted(n: number): string {
  return n === 1 ? '1 photo' : `${n.toLocaleString()} photos`;
}

/** The dialog that writes one keyword to a whole selection: which mode it is in, what is
 *  typed, which known keywords that narrows to, and the one-write-at-a-time guard.
 *
 *  Separated from the markup because there is no component harness here: everything with a
 *  decision in it lives in this factory and is tested, and what is left in `TagPicker.svelte`
 *  is focus wiring and layout.
 *
 *  The selection is captured at `show()` rather than read at submit time: the dialog takes
 *  focus, and the grid's selection behind it can be rebound by a scan landing meanwhile.
 *  What the user was told the dialog would act on is what it acts on. */
export function createTagPicker(deps: {
  apply: (mode: TagPickerMode, tag: string, ids: number[]) => Promise<TagWrite>;
}) {
  let visible = $state(false);
  let mode = $state<TagPickerMode>('add');
  let draft = $state('');
  let busy = $state(false);
  let ids: number[] = [];

  return {
    get visible(): boolean {
      return visible;
    },

    get mode(): TagPickerMode {
      return mode;
    },

    /** How many photos the write will reach, for the dialog's own title: the selection is
     *  behind the dialog and cannot be counted by eye once it has focus. */
    get count(): number {
      return ids.length;
    },

    get busy(): boolean {
      return busy;
    },

    get draft(): string {
      return draft;
    },

    set draft(value: string) {
      draft = value;
    },

    show(next: TagPickerMode, selection: number[]) {
      mode = next;
      ids = [...selection];
      draft = '';
      busy = false;
      visible = true;
    },

    close() {
      visible = false;
    },

    /** The known keywords this draft narrows to, best match first: the exact name, then the
     *  ones that start with it, then the rest that merely contain it. Enter writes the draft
     *  itself, so the first row must be what Enter would do or the two disagree.
     *
     *  Case-insensitive in TypeScript, like every other match in photon: there is no
     *  COLLATE NOCASE in the library and `lower()` is ASCII-only without ICU. */
    suggestions(all: string[]): string[] {
      const typed = draft.trim().toLowerCase();
      if (!typed) return [...all];
      const rank = (tag: string) => {
        const name = tag.toLowerCase();
        if (name === typed) return 0;
        if (name.startsWith(typed)) return 1;
        return 2;
      };
      return all
        .filter((t) => t.toLowerCase().includes(typed))
        .map((tag, at) => ({ tag, at, rank: rank(tag) }))
        // `at` keeps the library's own order (case-insensitive by name) within a rank,
        // since Array.prototype.sort is only stable by specification, not by every engine's
        // implementation of a comparator that returns 0.
        .sort((a, b) => a.rank - b.rank || a.at - b.at)
        .map((s) => s.tag);
    },

    /** Writes `tag` (or the draft) to the captured selection and returns the line to report,
     *  or null when there was nothing to do.
     *
     *  Closed before the write, not after: the write is the user's last word on this dialog,
     *  and leaving it up until the backend answers reads as a click that did nothing. `busy`
     *  is what stops a second Enter landing in that same frame - the dialog is still on
     *  screen until Svelte renders the close. */
    async submit(tag?: string): Promise<string | null> {
      const name = (tag ?? draft).trim();
      if (busy || !name || ids.length === 0) return null;
      busy = true;
      visible = false;
      const asked = ids.length;
      try {
        const done = await deps.apply(mode, name, ids);
        const verb = mode === 'add' ? 'Added' : 'Removed';
        const preposition = mode === 'add' ? 'to' : 'from';
        // The backend's count is the truth: a photo purged or gone missing since the grid
        // was built is skipped rather than refused, and swallowing the difference would
        // leave the user believing twelve photos carry a keyword ten of them do.
        const reached =
          done.count === asked ? counted(done.count) : `${done.count.toLocaleString()} of ${counted(asked)}`;
        return `${verb} “${done.tag}” ${preposition} ${reached}`;
      } finally {
        busy = false;
      }
    },
  };
}

export type TagPicker = ReturnType<typeof createTagPicker>;
