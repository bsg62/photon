/** Which item of an open menu a key moves the focus to, or null when the key is not one
 *  that moves it. `current` is the focused item's index among the menu's enabled items, -1
 *  while the focus is still on the menu itself, as it is when the menu opens.
 *
 *  The arrows wrap, as a native menu's do: Down from the last item is the first. From the
 *  menu itself Down is the first item and Up the last, so either arrow lands somewhere. */
export function menuStep(key: string, current: number, count: number): number | null {
  if (count <= 0) return null;
  switch (key) {
    case 'ArrowDown':
      return current < 0 ? 0 : (current + 1) % count;
    case 'ArrowUp':
      return current < 0 ? count - 1 : (current - 1 + count) % count;
    case 'Home':
      return 0;
    case 'End':
      return count - 1;
    default:
      return null;
  }
}

/** Whether a keydown inside an open menu is the menu's alone, and goes no further.
 *
 *  Every plain key is: the viewer listens on the window, and with its menu open an "h"
 *  hid the photo behind it and an arrow stepped past it. Escape is not - whoever opened the
 *  menu closes it, from the window - and neither is a chord, which is the app's (Ctrl+A,
 *  which must still be kept from selecting the whole window) or the system's. Nor a function
 *  key: F11 is the way out of a fullscreen photon reopened in, wherever the focus is. */
export function menuOwnsKey(e: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'altKey'>): boolean {
  return e.key !== 'Escape' && !/^F\d+$/.test(e.key) && !e.ctrlKey && !e.metaKey && !e.altKey;
}
