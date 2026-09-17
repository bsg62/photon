import type { TagCount, TagRule } from './api';
import { renameCheck } from './tags';

/** The Settings tag list's inline rename field, separated from the input that renders it.
 *
 *  The field stays open until a rename has actually been saved: a declined merge and a
 *  failed save both leave the typed name in place, with the backend's message under it
 *  for the second. `busy` covers the whole commit, confirm included, because the confirm
 *  dialog takes focus and the field's blur would otherwise cancel the edit it is asking
 *  about.
 *
 *  `rename`, `confirm` and `errorMessage` are injected so the machine can be tested
 *  without the backend or the dialog plugin. */
export function createTagRenamer(deps: {
  rename: (from: string, to: string) => Promise<unknown>;
  confirm: (message: string) => Promise<boolean>;
  errorMessage: (e: unknown) => string;
}) {
  let editing = $state<string | null>(null);
  let text = $state('');
  let error = $state('');
  let busy = $state(false);

  return {
    /** The tag whose field is open, or null. */
    get editing() {
      return editing;
    },
    get text() {
      return text;
    },
    set text(value: string) {
      text = value;
    },
    get error() {
      return error;
    },
    get busy() {
      return busy;
    },

    /** Opens the field seeded with the current name, so a small correction is a few
     *  keystrokes. */
    start(tag: string) {
      editing = tag;
      text = tag;
      error = '';
    },

    /** Escape and blur. Ignored while a commit is in flight; see above. */
    cancel() {
      if (busy) return;
      editing = null;
      error = '';
    },

    /** Resolves to whether a rename was saved; the field is closed exactly then, and when
     *  the name is unchanged. `existing` and `rules` are what the user can see, which is
     *  what decides whether to ask first. */
    async commit(existing: readonly TagCount[], rules: readonly TagRule[]): Promise<boolean> {
      const from = editing;
      if (busy || from === null) return false;
      const check = renameCheck(from, text, existing, rules);
      if (check === 'blank') {
        error = 'A tag needs a name.';
        return false;
      }
      if (check === 'same') {
        editing = null;
        error = '';
        return false;
      }
      const to = text.trim();
      busy = true;
      try {
        if (check === 'merge' || check === 'revive') {
          const message =
            check === 'merge'
              ? `Merge “${from}” into “${to}”? Photos tagged with either will show under “${to}”.`
              : `“${to}” is listed under Changes. Renaming onto it undoes that change and shows its photos and those tagged “${from}” under “${to}”.`;
          if (!(await deps.confirm(message))) return false;
        }
        await deps.rename(from, to);
      } catch (e) {
        error = deps.errorMessage(e);
        return false;
      } finally {
        busy = false;
      }
      editing = null;
      error = '';
      return true;
    },
  };
}

export type TagRenamer = ReturnType<typeof createTagRenamer>;
