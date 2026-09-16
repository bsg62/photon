/** The info panel's album checkboxes, separated from the inputs that render them.
 *
 *  Optimistic like the star toggle, for the same reason: a checkbox that lags its own click
 *  reads as broken. The revert on failure is guarded by the photo id, because the user can
 *  navigate while a call is in flight and a revert landing on the next photo would flip a
 *  box nobody touched.
 *
 *  `add` and `remove` are injected so the machine can be tested without the backend. */
export function createAlbumMembership(deps: {
  add: (albumId: number, itemIds: number[]) => Promise<void>;
  remove: (albumId: number, itemIds: number[]) => Promise<void>;
}) {
  let member = $state<Set<number>>(new Set());
  /** Albums with a call in flight; a second click on one of them is dropped, not queued. */
  let pending = $state<Set<number>>(new Set());
  let bound: number | null = null;

  return {
    /** Whether the bound photo is in `albumId`. */
    has(albumId: number): boolean {
      return member.has(albumId);
    },

    busy(albumId: number): boolean {
      return pending.has(albumId);
    },

    /** Called when the viewer loads a photo, with the albums it is in. */
    bind(itemId: number, albums: number[]) {
      bound = itemId;
      member = new Set(albums);
      pending = new Set();
    },

    /** Flips membership of the bound photo in `albumId`. Rejects with the backend's error
     *  after reverting, so the caller can report it. */
    async toggle(albumId: number): Promise<void> {
      if (bound === null || pending.has(albumId)) return;
      const id = bound;
      const joining = !member.has(albumId);
      const next = new Set(member);
      if (joining) next.add(albumId);
      else next.delete(albumId);
      member = next;
      pending = new Set(pending).add(albumId);
      try {
        if (joining) await deps.add(albumId, [id]);
        else await deps.remove(albumId, [id]);
      } catch (e) {
        if (bound === id) {
          const reverted = new Set(member);
          if (joining) reverted.delete(albumId);
          else reverted.add(albumId);
          member = reverted;
        }
        throw e;
      } finally {
        if (bound === id) {
          const done = new Set(pending);
          done.delete(albumId);
          pending = done;
        }
      }
    },
  };
}

export type AlbumMembership = ReturnType<typeof createAlbumMembership>;
