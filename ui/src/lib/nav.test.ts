import { describe, expect, it } from 'vitest';
import { clampPan, clampZoom, closesViewer, move, ownsSelectAll, positionInSection, wheelStep } from './nav';

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

