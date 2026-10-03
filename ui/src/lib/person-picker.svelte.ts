import type { NamedItems } from './api';
import { namedFaceMessage, namedItemsMessage, nameChoice } from './people';

export type PickerTarget = { kind: 'face'; face: number } | { kind: 'items'; items: number[] };

export interface PersonPickerDeps {
  nameFaces(faces: number[], name: string): Promise<number | null>;
  nameItems(items: number[], name: string): Promise<NamedItems>;
}

/** The dialog that names a face, or adds photos to a person: the target, what is typed,
 *  which known people that narrows to, and the one-write-at-a-time guard.
 *
 *  Like `createTagPicker`, the target is captured at `show()` (the grid's selection behind
 *  the dialog can be rebound meanwhile), and only `$state` and getters are used so the node
 *  test project can run it. */
export function createPersonPicker(deps: PersonPickerDeps) {
  let visible = $state(false);
  let busy = $state(false);
  let draft = $state('');
  let target = $state<PickerTarget | null>(null);

  return {
    get visible(): boolean {
      return visible;
    },
    get busy(): boolean {
      return busy;
    },
    get target(): PickerTarget | null {
      return target;
    },
    get draft(): string {
      return draft;
    },
    set draft(value: string) {
      draft = value;
    },

    /** What the write will touch, since the grid behind the dialog cannot be counted by eye. */
    get title(): string {
      if (target?.kind === 'items') {
        const n = target.items.length;
        return `Add ${n === 1 ? '1 photo' : `${n.toLocaleString()} photos`} to…`;
      }
      return 'Who is this?';
    },

    show(next: PickerTarget) {
      target = next.kind === 'items' ? { kind: 'items', items: [...next.items] } : { ...next };
      draft = '';
      busy = false;
      visible = true;
    },

    close() {
      visible = false;
    },

    /** Best match first: the exact name, then those starting with the draft, then the rest
     *  that contain it. Case-insensitive in TypeScript, as the backend's rule is in Rust. */
    suggestions(people: readonly { id: number; name: string }[]): { id: number; name: string }[] {
      const typed = draft.trim().toLowerCase();
      if (!typed) return [...people];
      const rank = (name: string) => {
        const n = name.toLowerCase();
        return n === typed ? 0 : n.startsWith(typed) ? 1 : 2;
      };
      return people
        .filter((p) => p.name.toLowerCase().includes(typed))
        .map((p, at) => ({ p, at, rank: rank(p.name) }))
        .sort((a, b) => a.rank - b.rank || a.at - b.at)
        .map((s) => s.p);
    },

    /** What Enter will do, from `nameChoice` so the hint and the backend agree. */
    hint(people: readonly { id: number; name: string | null }[]): string {
      const choice = nameChoice(draft, people);
      if (choice.kind === 'merge') return `Add to ${choice.name}`;
      if (choice.kind === 'new') return `New person “${choice.name}”`;
      return '';
    },

    /** Closed before the write, as the keyword dialog does; `busy` stops a second Enter in
     *  the frame before Svelte renders the close. */
    async submit(name?: string): Promise<string | null> {
      const typed = (name ?? draft).trim();
      if (busy || !typed || !target) return null;
      const asked = target;
      busy = true;
      visible = false;
      try {
        if (asked.kind === 'face') {
          const person = await deps.nameFaces([asked.face], typed);
          return person === null ? 'That face is no longer there.' : namedFaceMessage(typed);
        }
        return namedItemsMessage(await deps.nameItems(asked.items, typed));
      } finally {
        busy = false;
      }
    },

    /** Every key's propagation is stopped: the viewer listens for single letters on the
     *  window, and typing "h" in the name field would otherwise hide the photo. */
    keydown(e: Pick<KeyboardEvent, 'key' | 'stopPropagation' | 'preventDefault'>): 'close' | 'submit' | null {
      e.stopPropagation();
      if (e.key === 'Escape') {
        e.preventDefault();
        return 'close';
      }
      if (e.key === 'Enter') {
        e.preventDefault();
        return 'submit';
      }
      return null;
    },
  };
}

export type PersonPicker = ReturnType<typeof createPersonPicker>;
