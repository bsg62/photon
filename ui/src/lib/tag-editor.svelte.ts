/** The info panel's tag editor, separated from the inputs that render it.
 *
 *  Optimistic like the album checkboxes and the star, for the same reason: a chip that lags
 *  its own click reads as broken. The revert on failure is guarded by the photo id, because
 *  the user can navigate while a call is in flight and a revert landing on the next photo
 *  would change a tag nobody touched.
 *
 *  `add` resolves to the name the backend stored, which a rename rule can make different
 *  from what was typed; the optimistic chip is replaced with that name.
 *
 *  `add` and `remove` are injected so the machine can be tested without the backend. */
export function createTagEditor(deps: {
  add: (itemId: number, tag: string) => Promise<string>;
  remove: (itemId: number, tag: string) => Promise<void>;
}) {
  let tags = $state<string[]>([]);
  /** Tags with a call in flight; a second click on one of them is dropped, not queued. */
  let pending = $state<Set<string>>(new Set());
  /** What the user is typing. A `let` with accessors, not a property: `$state` is only
   *  valid in a variable declaration or a class field, never in an object literal. */
  let draft = $state('');
  let bound: number | null = null;

  return {
    get draft(): string {
      return draft;
    },

    set draft(value: string) {
      draft = value;
    },

    get list(): string[] {
      return tags;
    },

    busy(tag: string): boolean {
      return pending.has(tag);
    },

    /** Called when the viewer loads a photo, with the tags it carries. */
    bind(itemId: number, current: string[]) {
      bound = itemId;
      tags = [...current];
      pending = new Set();
      draft = '';
    },

    /** Known tags the photo does not carry, matching the draft. Case-insensitive here
     *  rather than in SQL: there is no COLLATE NOCASE in the library and `lower()` is
     *  ASCII-only without ICU, so the whole app matches case in the language, not the
     *  query. */
    suggestions(all: string[]): string[] {
      const typed = draft.trim().toLowerCase();
      const have = new Set(tags.map((t) => t.toLowerCase()));
      return all.filter((t) => !have.has(t.toLowerCase()) && t.toLowerCase().includes(typed));
    },

    /** Adds `tag` to the bound photo. A blank name, a name already shown, or one already in
     *  flight is dropped: each would be a call whose result the user can already see.
     *
     *  If the backend returns a stored name that is already in the list (typing `holiday`
     *  under `holiday → vacation` when the photo already shows `vacation`), the existing
     *  entry is removed and re-appended, so it silently moves to the end of the panel. No
     *  duplication, no data loss, and it resets at the next `bind` — accepted behaviour,
     *  not a bug. */
    async add(tag: string): Promise<void> {
      const name = tag.trim();
      if (bound === null || !name || pending.has(name)) return;
      if (tags.some((t) => t === name)) return;
      const id = bound;
      tags = [...tags, name];
      pending = new Set(pending).add(name);
      try {
        const stored = await deps.add(id, name);
        if (bound === id && stored !== name) {
          tags = tags.filter((t) => t !== name && t !== stored).concat(stored);
        }
      } catch (e) {
        if (bound === id) tags = tags.filter((t) => t !== name);
        throw e;
      } finally {
        if (bound === id) {
          const done = new Set(pending);
          done.delete(name);
          pending = done;
        }
      }
    },

    /** Removes `tag` from the bound photo, putting it back at its old position if the call
     *  fails — appending it would reorder the panel for no reason the user can see. */
    async remove(tag: string): Promise<void> {
      if (bound === null || pending.has(tag)) return;
      const id = bound;
      const at = tags.indexOf(tag);
      if (at < 0) return;
      tags = tags.filter((t) => t !== tag);
      pending = new Set(pending).add(tag);
      try {
        await deps.remove(id, tag);
      } catch (e) {
        if (bound === id) {
          const back = [...tags];
          back.splice(at, 0, tag);
          tags = back;
        }
        throw e;
      } finally {
        if (bound === id) {
          const done = new Set(pending);
          done.delete(tag);
          pending = done;
        }
      }
    },
  };
}

export type TagEditor = ReturnType<typeof createTagEditor>;
