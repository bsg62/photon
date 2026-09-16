/** The sidebar's inline album editor, separated from the input that renders it.
 *
 *  One editor serves both "New album…" and "Rename": the sidebar has room for exactly one
 *  text field at a time, and a rename started while a new album is half-typed replaces it
 *  rather than stacking. The commit is what talks to the backend, and the caller decides
 *  what to refetch afterwards.
 *
 *  `create` and `rename` are injected so the machine can be tested without the backend. */
export function createAlbumEditor(deps: {
  create: (name: string) => Promise<unknown>;
  rename: (albumId: number, name: string) => Promise<unknown>;
}) {
  type Mode = { kind: 'idle' } | { kind: 'new' } | { kind: 'rename'; albumId: number };
  let mode = $state<Mode>({ kind: 'idle' });
  let text = $state('');
  let busy = $state(false);

  return {
    get mode() {
      return mode;
    },
    get text() {
      return text;
    },
    set text(value: string) {
      text = value;
    },
    get busy() {
      return busy;
    },

    /** Whether the editor is showing a field for this album, or for a new one when
     *  `albumId` is undefined. */
    editing(albumId?: number): boolean {
      return albumId === undefined ? mode.kind === 'new' : mode.kind === 'rename' && mode.albumId === albumId;
    },

    startNew() {
      mode = { kind: 'new' };
      text = '';
    },

    /** Starts a rename seeded with the current name, so a small correction is a few
     *  keystrokes rather than retyping. */
    startRename(albumId: number, currentName: string) {
      mode = { kind: 'rename', albumId };
      text = currentName;
    },

    cancel() {
      mode = { kind: 'idle' };
      text = '';
    },

    /** Commits the field. A blank name is a cancel, not an error: Escape and an empty
     *  Enter should feel the same. Resolves to whether anything was sent. Rejects with the
     *  backend's error after leaving the field open, so the user can fix the name. */
    async commit(): Promise<boolean> {
      if (busy) return false;
      const name = text.trim();
      const current = mode;
      if (current.kind === 'idle') return false;
      if (!name) {
        this.cancel();
        return false;
      }
      busy = true;
      try {
        if (current.kind === 'new') await deps.create(name);
        else await deps.rename(current.albumId, name);
      } finally {
        busy = false;
      }
      mode = { kind: 'idle' };
      text = '';
      return true;
    },
  };
}

export type AlbumEditor = ReturnType<typeof createAlbumEditor>;
