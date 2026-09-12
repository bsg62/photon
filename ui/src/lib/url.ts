/** Tauri serves custom schemes as `scheme://localhost/…`, except on Windows (WebView2),
 *  where they are `http://scheme.localhost/…`. */
export function isWindows(): boolean {
  return typeof navigator !== 'undefined' && navigator.userAgent.includes('Windows');
}

/** URL for a path served by the `photon://` protocol, e.g. `thumb/12/grid/<key>`. */
export function mediaUrl(path: string, windows: boolean = isWindows()): string {
  return windows ? `http://photon.localhost/${path}` : `photon://localhost/${path}`;
}
