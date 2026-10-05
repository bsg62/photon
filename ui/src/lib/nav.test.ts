import { describe, expect, it } from 'vitest';
import {
  actualSizeZoom,
  clampPan,
  clampZoom,
  closesViewer,
  DOUBLE_CLICK_ZOOM,
  fieldOwnsKey,
  MAX_ZOOM,
  move,
  ownsSelectAll,
  positionInSection,
  wheelStep,
  wheelZoomFactor,
  zoomAt,
} from './nav';

const sections = [
  { folderId: 1, offset: 0, count: 5 },
  { folderId: 2, offset: 5, count: 3 },
];

describe('move', () => {
  it('steps left and right across the whole grid', () => {
    expect(move(0, 'ArrowLeft', sections, 2)).toBe(0);
    expect(move(4, 'ArrowRight', sections, 2)).toBe(5);
    expect(move(7, 'ArrowRight', sections, 2)).toBe(7);
    expect(move(3, 'Home', sections, 2)).toBe(0);
    expect(move(3, 'End', sections, 2)).toBe(7);
  });

  it('moves down by rows, into the next section at the same column', () => {
    expect(move(0, 'ArrowDown', sections, 2)).toBe(2);
    expect(move(3, 'ArrowDown', sections, 2)).toBe(4);
    expect(move(4, 'ArrowDown', sections, 2)).toBe(5);
    expect(move(6, 'ArrowDown', sections, 2)).toBe(7);
    expect(move(7, 'ArrowDown', sections, 2)).toBe(7);
  });

  it('moves up by rows, into the previous section’s last row', () => {
    expect(move(3, 'ArrowUp', sections, 2)).toBe(1);
    expect(move(5, 'ArrowUp', sections, 2)).toBe(4);
    expect(move(6, 'ArrowUp', sections, 2)).toBe(4);
    expect(move(1, 'ArrowUp', sections, 2)).toBe(1);
  });

  it('handles an empty grid', () => {
    expect(move(0, 'ArrowDown', [], 4)).toBe(0);
  });
});

describe('positionInSection', () => {
  it('numbers a photo within its own folder, not the whole library', () => {
    expect(positionInSection(sections, 0)).toEqual({ index: 1, count: 5 });
    expect(positionInSection(sections, 4)).toEqual({ index: 5, count: 5 });
    // The first photo of the second folder restarts at 1 rather than continuing at 6.
    expect(positionInSection(sections, 5)).toEqual({ index: 1, count: 3 });
    expect(positionInSection(sections, 7)).toEqual({ index: 3, count: 3 });
  });

  it('reports nothing for an empty grid', () => {
    expect(positionInSection([], 0)).toEqual({ index: 0, count: 0 });
  });
});

describe('positionInSection past the end', () => {
  it('never counts past the section it lands in', () => {
    // The viewer's offset is not clamped by a rescan that shrinks the library, so it can
    // outrun the sections until the next keypress. "31 / 12" is not a number to show.
    expect(positionInSection(sections, 40)).toEqual({ index: 3, count: 3 });
  });
});

describe('move from no selection', () => {
  it('honours the key when nothing is selected yet', () => {
    // Clicking the grid background clears the selection. Treating every navigation key as
    // "select the first photo" from there sent End and ArrowUp to the top of the library.
    expect(move(null, 'End', sections, 4)).toBe(7);
    expect(move(null, 'Home', sections, 4)).toBe(0);
    expect(move(null, 'ArrowRight', sections, 4)).toBe(1);
  });

  it('moves from the current selection when there is one', () => {
    expect(move(3, 'ArrowRight', sections, 4)).toBe(4);
    expect(move(3, 'End', sections, 4)).toBe(7);
  });
});

describe('wheelStep', () => {
  it('holds until the accumulated delta passes the threshold', () => {
    expect(wheelStep(0, 30, 100)).toEqual({ accumulated: 30, step: 0 });
    expect(wheelStep(90, 30, 100)).toEqual({ accumulated: 0, step: 1 });
    expect(wheelStep(0, -120, 100)).toEqual({ accumulated: 0, step: -1 });
  });

  it('rate-limits a trackpad’s stream of small deltas', () => {
    // A flick arrives as many small events. Without accumulation each one would advance a
    // photo and a single gesture would fly through the folder.
    let accumulated = 0;
    let steps = 0;
    for (let i = 0; i < 10; i++) {
      const r = wheelStep(accumulated, 60, 100);
      accumulated = r.accumulated;
      steps += Math.abs(r.step);
    }
    expect(steps).toBe(5);
  });

  it('resets the accumulator when the wheel changes direction', () => {
    // Otherwise a banked-up scroll one way would fire a step the moment you reverse.
    expect(wheelStep(90, -30, 100)).toEqual({ accumulated: -30, step: 0 });
    expect(wheelStep(-90, 30, 100)).toEqual({ accumulated: 30, step: 0 });
  });
});

describe('clampZoom', () => {
  it('keeps zoom between fit and 400%', () => {
    expect(clampZoom(0.2)).toBe(1);
    expect(clampZoom(1)).toBe(1);
    expect(clampZoom(2.5)).toBe(2.5);
    expect(clampZoom(4)).toBe(4);
    expect(clampZoom(9)).toBe(4);
  });
});

describe('clampPan', () => {
  it('allows panning only as far as the scaled photo overflows the viewport', () => {
    // At 2x an 800x600 viewport, the photo is 1600x1200, so half of the overflow —
    // 400 across and 300 down — is as far as either edge can travel.
    expect(clampPan(1000, 1000, 2, 800, 600)).toEqual({ x: 400, y: 300 });
    expect(clampPan(-1000, -1000, 2, 800, 600)).toEqual({ x: -400, y: -300 });
    expect(clampPan(100, 50, 2, 800, 600)).toEqual({ x: 100, y: 50 });
  });

  it('pins the photo to the centre at fit, where there is nothing to pan', () => {
    expect(clampPan(250, 250, 1, 800, 600)).toEqual({ x: 0, y: 0 });
  });
});

describe('zoomAt', () => {
  it('keeps the photo under the pointer where it is', () => {
    // At fit, the pointer 200 right and 100 below the centre is over the fitted photo's
    // point (200, 100). At 2x that point is drawn at pan + 2 * (200, 100), so the pan that
    // leaves it under the pointer is (-200, -100).
    expect(zoomAt(1, { x: 0, y: 0 }, 2, { x: 200, y: 100 }, 800, 600)).toEqual({ zoom: 2, pan: { x: -200, y: -100 } });
    // From a photo already zoomed and panned: the point under the pointer is
    // (100 - 40) / 2 = 30, and at 3x it must still be drawn at 100.
    expect(zoomAt(2, { x: 40, y: 0 }, 3, { x: 100, y: 0 }, 800, 600)).toEqual({ zoom: 3, pan: { x: 10, y: 0 } });
  });

  it('zooms about the middle of the window when no point is given', () => {
    // The pan scales with the zoom, so what is at the centre stays there.
    expect(zoomAt(2, { x: 100, y: -50 }, 4, { x: 0, y: 0 }, 800, 600)).toEqual({ zoom: 4, pan: { x: 200, y: -100 } });
  });

  it('never leaves the photo short of the window', () => {
    // Zooming out from a corner: scaled about the centre the pan would be (600, 450), past
    // what a 2x photo can travel. It slides back in rather than leaving black beside it.
    expect(zoomAt(4, { x: 1200, y: 900 }, 2, { x: 0, y: 0 }, 800, 600).pan).toEqual({ x: 400, y: 300 });
    // And zooming out under a pointer at the far side, which asks for more still.
    expect(zoomAt(4, { x: 1200, y: 0 }, 2, { x: -400, y: 0 }, 800, 600).pan).toEqual({ x: 400, y: 0 });
  });

  it('is the centred fit at the bottom of the range, whatever the point', () => {
    expect(zoomAt(3, { x: 250, y: -90 }, 1, { x: 300, y: 200 }, 800, 600)).toEqual({ zoom: 1, pan: { x: 0, y: 0 } });
    expect(zoomAt(3, { x: 250, y: -90 }, 0.2, { x: 300, y: 200 }, 800, 600)).toEqual({ zoom: 1, pan: { x: 0, y: 0 } });
  });

  it('stops at the top of the range, and pans for the zoom it stopped at', () => {
    // Asked for 8x, it gets 4x - and the pan is 4x's, not 8x's clamped: the point under the
    // pointer has to stay there at the zoom actually reached.
    expect(zoomAt(2, { x: 0, y: 0 }, 8, { x: 100, y: 0 }, 800, 600)).toEqual({ zoom: MAX_ZOOM, pan: { x: -100, y: 0 } });
  });
});

describe('actualSizeZoom', () => {
  it('is one photo pixel on one screen pixel', () => {
    // 4000 wide, drawn 1600 wide at fit: 2.5x shows it pixel for pixel.
    expect(actualSizeZoom(4000, 1600, 1)).toBe(2.5);
    // A display that draws two screen pixels per CSS pixel needs half the zoom.
    expect(actualSizeZoom(6400, 1600, 2)).toBe(2);
  });

  it('stops at the most the viewer zooms to', () => {
    expect(actualSizeZoom(6000, 1000, 1)).toBe(MAX_ZOOM);
  });

  it('still zooms a photo that has no more pixels to show', () => {
    // Smaller than its fitted box, exactly its size, or a hair over: a double-click that
    // moved nothing, or by a few percent, would read as broken.
    expect(actualSizeZoom(800, 1600, 1)).toBe(DOUBLE_CLICK_ZOOM);
    expect(actualSizeZoom(1600, 1600, 1)).toBe(DOUBLE_CLICK_ZOOM);
    expect(actualSizeZoom(1700, 1600, 1)).toBe(DOUBLE_CLICK_ZOOM);
  });

  it('still zooms before anything has been measured', () => {
    expect(actualSizeZoom(0, 1600, 1)).toBe(DOUBLE_CLICK_ZOOM);
    expect(actualSizeZoom(4000, 0, 1)).toBe(DOUBLE_CLICK_ZOOM);
    expect(actualSizeZoom(4000, 1600, 0)).toBe(DOUBLE_CLICK_ZOOM);
  });
});

describe('wheelZoomFactor', () => {
  it('gives a pinch back as it was made', () => {
    // A pinch to 1.1x arrives as a delta of -100 * ln(1.1).
    expect(wheelZoomFactor(-100 * Math.log(1.1))).toBeCloseTo(1.1, 10);
    expect(wheelZoomFactor(100 * Math.log(1.1))).toBeCloseTo(1 / 1.1, 10);
  });

  it('makes a mouse notch a step of about a fifth, not nearly three times', () => {
    expect(wheelZoomFactor(-100)).toBeCloseTo(Math.exp(0.2), 10);
    expect(wheelZoomFactor(100)).toBeCloseTo(Math.exp(-0.2), 10);
    // However hard the wheel is flicked.
    expect(wheelZoomFactor(-1200)).toBe(wheelZoomFactor(-100));
  });

  it('ends where it began after the same way out and back', () => {
    expect(wheelZoomFactor(-7) * wheelZoomFactor(7)).toBeCloseTo(1, 12);
  });

  it('changes nothing for a delta of nothing, or one that is not a number', () => {
    expect(wheelZoomFactor(0)).toBe(1);
    expect(wheelZoomFactor(Number.NaN)).toBe(1);
    expect(wheelZoomFactor(Number.POSITIVE_INFINITY)).toBe(1);
  });
});

describe('fieldOwnsKey', () => {
  it('leaves a slider the keys that move it, and no others', () => {
    for (const key of ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End', 'PageUp', 'PageDown']) {
      expect(fieldOwnsKey({ type: 'range' }, key), key).toBe(true);
    }
    // The viewer's own: zoom, the star, information, the slideshow, the way out.
    for (const key of ['0', '+', '-', '.', 'i', 's', ' ', 'Backspace']) {
      expect(fieldOwnsKey({ type: 'range' }, key), key).toBe(false);
    }
  });

  it('leaves every key to a field that is typed in or ticked', () => {
    for (const type of ['text', 'search', 'checkbox', '']) {
      expect(fieldOwnsKey({ type }, 'i'), type).toBe(true);
      expect(fieldOwnsKey({ type }, 'Backspace'), type).toBe(true);
      expect(fieldOwnsKey({ type }, 'ArrowLeft'), type).toBe(true);
    }
  });
});

describe('closesViewer', () => {
  it('closes on the back button', () => {
    expect(closesViewer(3)).toBe(true);
  });

  it('leaves the ordinary buttons alone', () => {
    // Left pans the photo and right opens the context menu; claiming either here would
    // shut the viewer out from under a drag.
    expect(closesViewer(0)).toBe(false);
    expect(closesViewer(1)).toBe(false);
    expect(closesViewer(2)).toBe(false);
  });

  it('does not claim the forward button', () => {
    // Forward has nowhere to go — photon keeps no history — so swallowing it would make a
    // stray press on a five-button mouse close the viewer for no stated reason.
    expect(closesViewer(4)).toBe(false);
  });
});

describe('ownsSelectAll', () => {
  it('leaves a text field to the browser', () => {
    // Ctrl+A in the search box means "select this query", and in Settings' rename fields
    // "select this name". Swallowing it there would break an editing key every text field
    // in every application has.
    expect(ownsSelectAll({ tagName: 'INPUT' })).toBe(false);
    expect(ownsSelectAll({ tagName: 'input' })).toBe(false);
    expect(ownsSelectAll({ tagName: 'TEXTAREA' })).toBe(false);
    expect(ownsSelectAll({ tagName: 'DIV', isContentEditable: true })).toBe(false);
  });

  it('claims it everywhere else', () => {
    // The webview's own Ctrl+A paints the whole application with a selection highlight —
    // the sidebar, the status bar, every folder name — which is what a browser does to a
    // page and what a photo manager must not do to its own chrome.
    expect(ownsSelectAll({ tagName: 'DIV' })).toBe(true);
    expect(ownsSelectAll({ tagName: 'BUTTON' })).toBe(true);
    expect(ownsSelectAll({ tagName: 'BODY' })).toBe(true);
    expect(ownsSelectAll({ tagName: 'SPAN', isContentEditable: false })).toBe(true);
  });

  it('claims a keystroke whose target has gone', () => {
    // An element removed mid-keystroke leaves a null target; the keystroke is still one the
    // application, not the webview, answers for.
    expect(ownsSelectAll(null)).toBe(true);
  });
});

