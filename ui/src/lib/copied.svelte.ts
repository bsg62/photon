/** How long the caption reads "Copied" after a click. Long enough to be noticed, short
 *  enough that the file name is back before anyone wonders where it went. */
export const COPIED_MS = 1200;

/** The state behind click-to-copy on the viewer's caption: writes through `write` and shows
 *  a confirmation for `COPIED_MS`. A `.svelte.ts` factory rather than component state so the
 *  timing is tested with fake timers; the component only reads `copied`. */
export function createCopyFeedback(write: (text: string) => Promise<void>) {
  let copied = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  return {
    get copied() {
      return copied;
    },

    /** Copies `text`. Rejects, with nothing shown, when the clipboard refuses: the caller
     *  reports that the way it reports any other failure. */
    async copy(text: string): Promise<void> {
      await write(text);
      copied = true;
      if (timer !== undefined) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = undefined;
        copied = false;
      }, COPIED_MS);
    },

    /** For the component's teardown: a timer must not fire into an unmounted component. */
    dispose() {
      if (timer !== undefined) clearTimeout(timer);
      timer = undefined;
    },
  };
}
