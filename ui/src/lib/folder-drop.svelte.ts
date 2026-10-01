import { errorMessage, type FileDrag, type WatchedFolder } from './api';

/** The last component of a path, for a message: "Holiday", not the whole way to it. */
function leaf(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/** Folders dropped on the window become watched folders.
 *
 *  The decisions live here rather than in `App.svelte` for the usual reason: there is no
 *  component harness. The backend call, the folder refresh and the two kinds of toast are
 *  injected.
 *
 *  Every dropped path is offered to the backend, which is what knows a folder from a file,
 *  a folder already watched (answered as itself, not as an error) and one that overlaps
 *  another. The drop is reported once: what is now watched, and the first refusal with a
 *  count of the rest - a drop of forty photos must not be forty toasts. */
export function createFolderDrop(deps: {
  add: (path: string) => Promise<WatchedFolder>;
  /** Re-reads the folder list, once, after a drop that added something. */
  refresh: () => Promise<void>;
  notify: (message: string) => void;
  reportError: (error: unknown) => void;
}) {
  let hovering = $state(false);

  async function drop(paths: string[]): Promise<void> {
    const added: WatchedFolder[] = [];
    const refused: unknown[] = [];
    // One at a time: two folders of one drop can overlap each other, and the backend can
    // only refuse the second if it has finished the first.
    for (const path of paths) {
      try {
        added.push(await deps.add(path));
      } catch (e) {
        refused.push(e);
      }
    }
    if (added.length > 0) {
      await deps.refresh().catch(deps.reportError);
      deps.notify(added.length === 1 ? `Watching “${leaf(added[0].path)}”` : `Watching ${added.length} folders`);
    }
    if (refused.length > 0) {
      // A refusal from the backend is `{ kind, message }`, not an `Error`.
      const first = refused[0];
      deps.reportError(
        refused.length === 1 ? first : new Error(`${errorMessage(first)} (and ${refused.length - 1} more)`),
      );
    }
  }

  return {
    /** Whether something is being dragged over the window, for the overlay that says what
     *  dropping it will do. */
    get hovering(): boolean {
      return hovering;
    },

    /** Feeds one step of the drag. A drag that carries no paths - text dragged out of
     *  another program - is not ours to announce. */
    handle(event: FileDrag): Promise<void> {
      if (event.type === 'enter') {
        hovering = event.paths.length > 0;
        return Promise.resolve();
      }
      hovering = false;
      return event.type === 'drop' && event.paths.length > 0 ? drop(event.paths) : Promise.resolve();
    },
  };
}
