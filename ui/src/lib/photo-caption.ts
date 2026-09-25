/** The caption line under the photo in the viewer: the photo's own caption on one line.
 *  Not `caption.ts`, which builds the viewer's file-name line - the two are different texts
 *  that happen to share a word. Pure, so each rule is pinned by a test. */

/** Line breaks and runs of whitespace become single spaces, because the line under the photo
 *  is clamped to two lines and a break would spend one of them on nothing; the info panel
 *  shows the caption with its breaks. `null` means draw nothing - not an empty strip. */
export function photoCaptionLine(caption: string | null): string | null {
  const line = (caption ?? '').replace(/\s+/g, ' ').trim();
  return line === '' ? null : line;
}
