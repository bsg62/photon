/** Whether a keypress copies the photo - the one on screen in the viewer, or the one
 *  selected in the grid. Pure, so the rules are pinned by a test. */

export interface CopyKey {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

/** Ctrl+C, or Cmd+C on a Mac - either modifier on any platform, as the grid's other
 *  shortcuts take them. Not while text is selected: someone who selected a caption or a path
 *  in the info panel is copying that text, and the webview's own copy must get the key.
 *  Plain C is the viewer's crop key, and Shift or Alt make it some other shortcut. */
export function isCopyPhotoShortcut(e: CopyKey, hasTextSelection: boolean): boolean {
  return (
    (e.ctrlKey || e.metaKey) &&
    !e.altKey &&
    !e.shiftKey &&
    e.key.toLowerCase() === 'c' &&
    !hasTextSelection
  );
}
