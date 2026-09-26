/** Where a poster frame is taken: a second in, since the first frame is often black, or a
 *  tenth of a clip shorter than ten seconds. An unknown duration (a WebM without one) takes
 *  the start. */
export function posterTime(duration: number): number {
  return Number.isFinite(duration) && duration > 0 ? Math.min(1, duration / 10) : 0;
}

export function formatDuration(ms: number): string {
  const total = Math.floor(ms / 1000);
  const [h, m, s] = [Math.floor(total / 3600), Math.floor(total / 60) % 60, total % 60];
  const ss = String(s).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}

/** Whether this webview can play video at all. On Linux this is also the guard against a
 *  crash: GStreamer's MP4 demuxer ships in gst-plugins-good, the same package as the
 *  `autoaudiosink` whose absence aborts WebKit's web process on any `<video>` load - so
 *  "MP4 is supported" proves the crash cannot happen. Do not swap in a codec that lives in
 *  another package. `canPlayType` itself is safe without the plugins (measured). */
export function mediaSupported(canPlayType: (type: string) => string): boolean {
  return canPlayType('video/mp4') !== '';
}

export function videoUrl(base: string, id: number): string {
  return `${base}/video/${id}`;
}

/** `ThumbSize::Preview.max_edge()` in `thumbs/cache.rs`; the backend shrinks anything
 *  larger, so this only saves the IPC bytes. */
export const PREVIEW_MAX_EDGE = 1600;

/** The webview has no decoder for this file: a fact about the platform, not the file. */
export class MediaUnsupported extends Error {}
/** The webview tried and failed: a fact about the file. */
export class MediaDecodeError extends Error {}

/** Refuses a video whose metadata loaded without a picture: `videoWidth`/`videoHeight` read
 *  0 when the file has no video track the webview can decode - an audio-only `.mp4`, or HEVC
 *  on Windows without Microsoft's extension. Drawn anyway it would be a 1x1 black frame,
 *  stored as the poster and marked Ready, and nothing would ever draw it again. As
 *  `MediaUnsupported` it stays Pending and is retried next session, so installing the codec
 *  later still fills the poster in. */
export function requirePicture(width: number, height: number): void {
  if (!(width > 0 && height > 0)) throw new MediaUnsupported('no picture');
}
