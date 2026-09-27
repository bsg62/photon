import { describe, expect, it } from 'vitest';
import { singleFlight } from './single-flight';

function deferred() {
  let resolve!: () => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

/** A `run` whose every call is held until the test releases it. */
function held() {
  const runs: ReturnType<typeof deferred>[] = [];
  const run = () => {
    const d = deferred();
    runs.push(d);
    return d.promise;
  };
  return { runs, run };
}

describe('singleFlight', () => {
  it('queues one run behind the one in flight, shared by every caller arriving meanwhile', async () => {
    const { runs, run } = held();
    const call = singleFlight(run);
    const first = call();
    let queuedDone = 0;
    const later = [call(), call(), call()].map((p) => p.then(() => queuedDone++));
    expect(runs).toHaveLength(1);

    runs[0].resolve();
    await first;
    await flush();
    // Not folded into the run in flight: it may have read before these callers' reason.
    expect(queuedDone).toBe(0);
    expect(runs).toHaveLength(2);

    runs[1].resolve();
    await Promise.all(later);
    expect(queuedDone).toBe(3);
    expect(runs).toHaveLength(2);
  });

  it('runs the queued call after a failed one, and rejects only the failed one’s callers', async () => {
    const { runs, run } = held();
    const call = singleFlight(run);
    const first = call();
    const second = call();
    runs[0].reject(new Error('boom'));
    await expect(first).rejects.toThrow('boom');
    await flush();
    expect(runs).toHaveLength(2);
    runs[1].resolve();
    await expect(second).resolves.toBeUndefined();
  });

  it('skips the queued run when every caller sharing it is satisfied by then', async () => {
    const { runs, run } = held();
    const call = singleFlight(run);
    let version = 1;
    void call();
    const satisfied = call(() => version >= 2);
    const alsoSatisfied = call(() => version >= 2);

    // The run in flight brought version 2.
    version = 2;
    runs[0].resolve();
    await Promise.all([satisfied, alsoSatisfied]);
    expect(runs).toHaveLength(1);

    // Already satisfied at the call: no run at all.
    await call(() => version >= 2);
    expect(runs).toHaveLength(1);

    // One caller without a condition is enough to make the queued run happen.
    void call();
    const conditional = call(() => version >= 2);
    const plain = call();
    runs[1].resolve();
    await flush();
    expect(runs).toHaveLength(3);
    runs[2].resolve();
    await Promise.all([conditional, plain]);
  });
});
