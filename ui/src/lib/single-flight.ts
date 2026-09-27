/** At most one `run` in flight, plus one queued behind it that every caller arriving
 *  meanwhile shares.
 *
 *  A caller resolves only once a run *started after its call* has finished: the one in
 *  flight when it called may have read the backend before whatever the caller is reacting to,
 *  so it waits for the queued one instead. That is what lets the view chain `await` a refresh
 *  and then trust `info` to show the view it just switched to.
 *
 *  `satisfied` lets a caller say what it is waiting for, when that can be answered without a
 *  fetch - "the grid at version 7 or later". It is asked when the call is made and again when
 *  the queued run is due; if every caller sharing the queued run is satisfied by then (the run
 *  that was in flight brought what they wanted), the run is skipped. A caller that passes none
 *  always gets its run.
 *
 *  A failed run rejects its own callers and does not stop the queued one. */
export function singleFlight(run: () => Promise<void>): (satisfied?: () => boolean) => Promise<void> {
  let running: Promise<void> | null = null;
  let queued: { waiting: (() => boolean)[]; done: Promise<void> } | null = null;

  const start = (): Promise<void> => {
    const current: Promise<void> = run().finally(() => {
      if (running === current) running = null;
    });
    running = current;
    return current;
  };

  return (satisfied = () => false) => {
    if (satisfied()) return Promise.resolve();
    if (queued) {
      queued.waiting.push(satisfied);
      return queued.done;
    }
    if (!running) return start();
    const next: { waiting: (() => boolean)[]; done: Promise<void> } = {
      waiting: [satisfied],
      done: running.then(
        () => {},
        () => {},
      ).then(() => {
        queued = null;
        if (next.waiting.every((s) => s())) return;
        return start();
      }),
    };
    queued = next;
    return next.done;
  };
}
