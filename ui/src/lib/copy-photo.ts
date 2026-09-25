/** Whether a keypress copies the photo - the one on screen in the viewer, or the one
 *  selected in the grid. Pure, so the rules are pinned by a test. */

export interface CopyKey {
  key: string;
  code: string;
  repeat: boolean;
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
    // A held key repeats: each repeat would queue another full-size decode behind the one
    // render lock the viewer's own edited photos wait on.
    !e.repeat &&
    isTheCKey(e) &&
    !hasTextSelection
  );
}

/** The C key: by the letter it types, or - when the layout types no Latin letter there
 *  (Cyrillic, Greek) - by its position, since Ctrl+C still means copy on those layouts.
 *  Never by position when the key types a Latin letter: on Dvorak that position is J, and
 *  Ctrl+J is not copy. */
function isTheCKey(e: CopyKey): boolean {
  const key = e.key.toLowerCase();
  return key === 'c' || (!/^[a-z]$/.test(key) && e.code === 'KeyC');
}
