import { MediaDecodeError, MediaUnsupported, posterTime } from './video';

/** Loads `url` into a hidden, muted video, seeks to the poster time, and returns the frame
 *  as a JPEG no larger than `maxEdge`. The element is attached while it works - as it was in
 *  the spike that measured this - and always unloaded after, which releases the pipeline. */
export async function grabPoster(url: string, signal: AbortSignal, maxEdge: number): Promise<Uint8Array> {
  const v = document.createElement('video');
  v.muted = true;
  v.preload = 'metadata';
  v.crossOrigin = 'anonymous';
  v.className = 'poster-grab';
  document.body.append(v);
  try {
    await until(v, 'loadedmetadata', signal, () => (v.src = url));
    await until(v, 'seeked', signal, () => (v.currentTime = posterTime(v.duration)));
    const scale = Math.min(1, maxEdge / Math.max(v.videoWidth, v.videoHeight, 1));
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(v.videoWidth * scale));
    canvas.height = Math.max(1, Math.round(v.videoHeight * scale));
    canvas.getContext('2d')?.drawImage(v, 0, 0, canvas.width, canvas.height);
    const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/jpeg', 0.9));
    if (!blob) throw new MediaDecodeError('no frame');
    return new Uint8Array(await blob.arrayBuffer());
  } finally {
    v.removeAttribute('src');
    v.load();
    v.remove();
  }
}

/** A rejection here becomes the job's failure reason, so it has to tell "the file is fine but
 *  the platform can't decode it" from "this file is broken". NETWORK (2, the loopback server
 *  went away - an offline folder, a video deleted mid-job) and ABORTED (1, the load was
 *  cancelled) are about reaching the file, not about it, so both are `MediaUnsupported`: that
 *  keeps the row Pending and skips it for the session rather than marking a fine video
 *  undecodable forever. SRC_NOT_SUPPORTED (4) is the platform's own "I don't have a decoder
 *  for this", also `MediaUnsupported`. Only DECODE (3) - the platform tried and the bytes
 *  beat it - is a fact about the file, `MediaDecodeError`. */
function until(v: HTMLVideoElement, event: string, signal: AbortSignal, begin: () => void): Promise<void> {
  return new Promise((resolve, reject) => {
    const done = () => {
      v.removeEventListener(event, ok);
      v.removeEventListener('error', bad);
      signal.removeEventListener('abort', aborted);
    };
    const ok = () => (done(), resolve());
    const bad = () => {
      done();
      const code = v.error?.code;
      const unsupported = code === MediaError.MEDIA_ERR_NETWORK || code === MediaError.MEDIA_ERR_ABORTED || code === MediaError.MEDIA_ERR_SRC_NOT_SUPPORTED;
      reject(unsupported ? new MediaUnsupported() : new MediaDecodeError(v.error?.message ?? 'error'));
    };
    const aborted = () => (done(), reject(signal.reason));
    v.addEventListener(event, ok);
    v.addEventListener('error', bad);
    signal.addEventListener('abort', aborted);
    begin();
  });
}
