/** Sidebar width, as set by dragging the splitter to its right. */

export const SIDEBAR_DEFAULT = 260;
export const SIDEBAR_MIN = 160;
/** Keyboard step for the focused splitter. */
export const SIDEBAR_STEP = 16;

/** Clamps a requested width to [SIDEBAR_MIN, half the window]. The minimum wins when the
 *  window is too narrow for both, so the folder list never collapses to nothing. */
export function clampSidebarWidth(width: number, viewport: number): number {
  return Math.round(Math.max(SIDEBAR_MIN, Math.min(width, viewport / 2)));
}
