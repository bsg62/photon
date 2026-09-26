/** Tauri serves custom schemes as `scheme://localhost/…`, except on Windows (WebView2),
 *  where they are `http://scheme.localhost/…`. */
export function isWindows(): boolean {
  return typeof navigator !== 'undefined' && navigator.userAgent.includes('Windows');
}

/** GStreamer is the Linux-only reason a video might not play (its `good`/`libav` plugins
 *  missing); a WebView2 or WebKit codec gap on Windows or macOS has no such fix, so the
 *  message for those has to be a plain "can't be played" rather than an install instruction. */
export function isLinux(): boolean {
  return typeof navigator !== 'undefined' && navigator.userAgent.includes('Linux');
}

/** URL for a path served by the `photon://` protocol, e.g. `thumb/12/grid/<key>`. */
export function mediaUrl(path: string, windows: boolean = isWindows()): string {
  return windows ? `http://photon.localhost/${path}` : `photon://localhost/${path}`;
}
