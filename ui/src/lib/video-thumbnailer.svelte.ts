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
 *  long as it runs. Generation-counted, so a stop and a start in quick succession never
 *  leave two loops. */
export function createVideoThumbnailer(deps: ThumbnailerDeps) {
  let generation = 0;
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
    const timer = setTimeout(() => controller.abort(new DOMException('timeout', 'TimeoutError')), JOB_TIMEOUT_MS);
    try {
      const jpeg = await deps.grab(deps.url(job.id), controller.signal);
      await deps.put(job.id, job.key, jpeg);
    } catch (e) {
      const reason: VideoFailure = controller.signal.aborted ? 'timeout' : e instanceof MediaUnsupported ? 'unsupported' : 'decode';
      await deps.fail(job.id, job.key, reason).catch(() => {});
    } finally {
      clearTimeout(timer);
    }
  }

  return {
    start() {
      void loop(++generation);
    },
    stop() {
      generation++;
    },
  };
}
