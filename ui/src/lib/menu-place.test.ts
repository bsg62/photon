import { afterEach, describe, expect, it, vi } from 'vitest';
import { fitMenu, MENU_MARGIN, placeMenu } from './menu-place';

const viewport = { width: 1000, height: 800 };
const menu = { width: 200, height: 150 };

describe('placeMenu', () => {
  it('opens down and to the right of the pointer when it fits', () => {
    expect(placeMenu({ x: 100, y: 100 }, menu, viewport)).toEqual({ x: 100, y: 100 });
  });

  it('grows upward from the pointer near the bottom edge', () => {
    expect(placeMenu({ x: 100, y: 700 }, menu, viewport)).toEqual({ x: 100, y: 550 });
  });

  it('opens to the left of the pointer near the right edge', () => {
    expect(placeMenu({ x: 900, y: 100 }, menu, viewport)).toEqual({ x: 700, y: 100 });
  });

  it('flips both ways in the bottom-right corner', () => {
    expect(placeMenu({ x: 990, y: 790 }, menu, viewport)).toEqual({ x: 790, y: 640 });
  });

  it('counts the margin when deciding to flip', () => {
    // 448 + 150 = 598 fits a 600-tall window only without the margin, so it opens upward.
    expect(placeMenu({ x: 10, y: 448 }, menu, { width: 1000, height: 600 })).toEqual({ x: 10, y: 298 });
  });

  it('pushes against the margin when neither side fits', () => {
    // 80 below the pointer and 120 above it: neither holds 150, so the flipped menu, which
    // would start above the window, is pushed down to the top margin - and still ends on screen.
    expect(placeMenu({ x: 10, y: 120 }, menu, { width: 1000, height: 200 })).toEqual({ x: 10, y: MENU_MARGIN });
  });

  it('pins a menu taller than the window to the top margin', () => {
    expect(placeMenu({ x: 10, y: 50 }, { width: 200, height: 900 }, viewport).y).toBe(MENU_MARGIN);
  });
});

describe('fitMenu', () => {
  afterEach(() => vi.unstubAllGlobals());

  /** A node whose measured size is whatever `size` says now, and the window's listeners and
   *  the ResizeObserver's callback captured so a test can fire them. */
  function setup(size: { width: number; height: number }) {
    const listeners: Record<string, () => void> = {};
    let observed: (() => void) | undefined;
    vi.stubGlobal('window', {
      innerWidth: 1000,
      innerHeight: 800,
      addEventListener: (type: string, f: () => void) => (listeners[type] = f),
      removeEventListener: (type: string) => delete listeners[type],
    });
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(f: () => void) {
          observed = f;
        }
        observe() {}
        disconnect() {
          observed = undefined;
        }
      },
    );
    const node = { style: { left: '', top: '' }, getBoundingClientRect: () => ({ ...size }) } as unknown as HTMLElement;
    return { node, listeners, resized: () => observed?.() };
  }

  it('places the menu when it opens, and again when it grows', () => {
    const size = { width: 200, height: 100 };
    const { node, resized } = setup(size);
    fitMenu(node, { x: 100, y: 650 });
    expect([node.style.left, node.style.top]).toEqual(['100px', '650px']);
    size.height = 200;
    resized();
    expect(node.style.top).toBe('450px');
  });

  it('measures its full width, not the width it is squeezed to at its old place', () => {
    // A fixed box shrinks to the room on its right; this node does the same, as a browser does.
    const { node, listeners } = setup({ width: 200, height: 100 });
    const squeezed = node as unknown as { style: { left: string }; getBoundingClientRect: () => { width: number; height: number } };
    squeezed.getBoundingClientRect = () => ({ width: Math.min(200, window.innerWidth - (parseFloat(squeezed.style.left) || 0)), height: 100 });
    fitMenu(node, { x: 700, y: 100 });
    expect(node.style.left).toBe('700px');
    // The window narrows to 800: at 700 the menu would measure 100 wide and be placed by that.
    vi.stubGlobal('window', { ...window, innerWidth: 800 });
    listeners.resize();
    expect(node.style.left).toBe('500px');
  });

  it('moves with the pointer on update, and follows a window resize until destroyed', () => {
    const { node, listeners } = setup({ width: 200, height: 100 });
    const handle = fitMenu(node, { x: 100, y: 100 });
    handle?.update?.({ x: 900, y: 100 });
    expect(node.style.left).toBe('700px');
    vi.stubGlobal('window', { ...window, innerWidth: 2000 });
    listeners.resize();
    expect(node.style.left).toBe('900px');
    handle?.destroy?.();
    expect(listeners.resize).toBeUndefined();
  });
});
