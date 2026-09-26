import type { VideoFailure, VideoJob } from './api';
import { MediaUnsupported } from './video';

/** A frame that has not come in this long is a failure, not a slow disk: the spike's 4K file
 *  gave its frame in 200-300 ms. */
export const JOB_TIMEOUT_MS = 15_000;
/** After an empty answer. The backend long-polls, so this only paces a backend that
 *  answers at once - an error, or the screenshot mock. */
export const IDLE_MS = 1_000;

export interface ThumbnailerDeps {
  nextJob: () => Promise<VideoJob | null>;
  put: (id: number, key: string, jpeg: Uint8Array) => Promise<void>;
  fail: (id: number, key: string, reason: VideoFailure) => Promise<void>;
  url: (id: number) => string;
  grab: (url: string, signal: AbortSignal) => Promise<Uint8Array>;
}

/** Draws videos' poster frames, one at a time - a WebKit media pipeline is heavy - for as
 *  long as it runs. Generation-counted, so a job answered late (`nextJob` was still
 *  in-flight when `stop()` ran) is never drawn.
 *
 *  `stop()` is a disposal, not a pause: once called, `start()` is a permanent no-op and
 *  the instance never runs again. `App.svelte` creates a fresh thumbnailer on every mount,
 *  the same lifecycle `theme.svelte.ts` documents for its own singleton, so nothing needs
 *  this one to come back to life. Treating it as resumable was the earlier bug: the async
 *  setup in `App.svelte` awaits `mediaBase()`/`videoSessionStart()` before calling
 *  `start()`, so `onMount`'s cleanup can run `stop()` first and `start()` can still land
 *  after - a generation bump alone would have let that late `start()` spin up a loop with
 *  nothing left to stop it. */
export function createVideoThumbnailer(deps: ThumbnailerDeps) {
  let generation = 0;
  let disposed = false;
  /** The controller for whichever `draw()` is in flight, so `stop()` can cut it short
   *  instead of leaving its `<video>` loading and its 15s timer armed until the tab-away
   *  page is long gone. */
  let active: AbortController | null = null;
  const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

  async function loop(mine: number) {
    while (mine === generation) {
      let job: VideoJob | null;
      try {
        job = await deps.nextJob();
      } catch {
        job = null;
      }
      if (mine !== generation) return;
      if (!job) {
        await sleep(IDLE_MS);
        continue;
      }
      await draw(job);
    }
  }

  async function draw(job: VideoJob) {
    const controller = new AbortController();
    active = controller;
    const timer = setTimeout(() => controller.abort(new DOMException('timeout', 'TimeoutError')), JOB_TIMEOUT_MS);
    try {
      let jpeg: Uint8Array;
      try {
        jpeg = await deps.grab(deps.url(job.id), controller.signal);
      } catch (e) {
        // A timeout and a stop both abort the same signal, but they are not the same fact:
        // a timeout means this video's frame did not come in a reasonable time, which is
        // worth remembering past this session. A stop means the page is going away mid-job -
        // the file is fine and untouched, so it is reported `unsupported`, the reason that
        // keeps the row Pending and lets a new session simply retry it, rather than marking a
        // healthy video as a repeat failure because the tab happened to close on it.
        const reason: VideoFailure = isTimeout(controller.signal)
          ? 'timeout'
          : isStop(controller.signal)
            ? 'unsupported'
            : e instanceof MediaUnsupported
              ? 'unsupported'
              : 'decode';
        await deps.fail(job.id, job.key, reason).catch(() => {});
        return;
      }
      try {
        await deps.put(job.id, job.key, jpeg);
      } catch {
        // The frame was drawn, so the file is not what failed: a full disk, or a body the
        // IPC layer refused, says nothing about the video, and `decode` here would fail a
        // healthy video for good - on a row already Ready if the frame was stored first.
        // `unsupported` keeps the row Pending for the next session, and it is sent rather
        // than nothing because a `put` refused before it reached the backend leaves this job
        // claimed: an unanswered claim is counted as a death when the next session starts,
        // and two of those fail the video as one that crashed the window.
        await deps.fail(job.id, job.key, 'unsupported').catch(() => {});
      }
    } finally {
      clearTimeout(timer);
      if (active === controller) active = null;
    }
  }

  return {
    start() {
      if (disposed) return;
      void loop(++generation);
    },
    stop() {
      disposed = true;
      generation++;
      active?.abort(new DOMException('stopped', 'AbortError'));
    },
  };
}

function isTimeout(signal: AbortSignal): boolean {
  return signal.aborted && signal.reason instanceof DOMException && signal.reason.name === 'TimeoutError';
}

function isStop(signal: AbortSignal): boolean {
  return signal.aborted && signal.reason instanceof DOMException && signal.reason.name === 'AbortError';
}
