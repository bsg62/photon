import { describe, expect, it } from 'vitest';
import { clampPan, clampZoom, closesViewer, move, nextSelection, positionInFolder, positionInView, wheelStep } from './nav';

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

describe('positionInFolder', () => {
  it('numbers a photo within its own folder, not the whole library', () => {
    expect(positionInFolder(sections, 0)).toEqual({ index: 1, count: 5 });
    expect(positionInFolder(sections, 4)).toEqual({ index: 5, count: 5 });
    // The first photo of the second folder restarts at 1 rather than continuing at 6.
    expect(positionInFolder(sections, 5)).toEqual({ index: 1, count: 3 });
    expect(positionInFolder(sections, 7)).toEqual({ index: 3, count: 3 });
  });

  it('reports nothing for an empty grid', () => {
    expect(positionInFolder([], 0)).toEqual({ index: 0, count: 0 });
  });
});

describe('positionInFolder past the end', () => {
  it('never counts past the section it lands in', () => {
    // The viewer's offset is not clamped by a rescan that shrinks the library, so it can
    // outrun the sections until the next keypress. "31 / 12" is not a number to show.
    expect(positionInFolder(sections, 40)).toEqual({ index: 3, count: 3 });
  });
});

describe('positionInView', () => {
  it('numbers within the folder in every folder-ordered view', () => {
    expect(positionInView('all', sections, 5, 8)).toEqual({ index: 1, count: 3 });
    expect(positionInView('starred', sections, 7, 8)).toEqual({ index: 3, count: 3 });
    expect(positionInView('search', sections, 0, 8)).toEqual({ index: 1, count: 5 });
  });

  it('numbers within the whole list in Recent', () => {
    // Recent orders by date across folders, so the index splits it into a section per photo
    // wherever folders interleave — and counting within the folder then answered "1 / 1"
    // for photo after photo. A flat list is counted flat.
    const perPhoto = [0, 1, 2].map((i) => ({ folderId: 10 + i, offset: i, count: 1 }));
    expect(positionInView('recent', perPhoto, 0, 3)).toEqual({ index: 1, count: 3 });
    expect(positionInView('recent', perPhoto, 2, 3)).toEqual({ index: 3, count: 3 });
  });

  it('reports nothing for an empty grid', () => {
    expect(positionInView('recent', [], 0, 0)).toEqual({ index: 0, count: 0 });
    expect(positionInView('all', [], 0, 0)).toEqual({ index: 0, count: 0 });
  });
});

describe('nextSelection', () => {
  it('honours the key when nothing is selected yet', () => {
    // Clicking the grid background clears the selection. Treating every navigation key as
    // "select the first photo" from there sent End and ArrowUp to the top of the library.
    expect(nextSelection(null, 'End', sections, 4)).toBe(7);
    expect(nextSelection(null, 'Home', sections, 4)).toBe(0);
    expect(nextSelection(null, 'ArrowRight', sections, 4)).toBe(1);
  });

  it('moves from the current selection when there is one', () => {
    expect(nextSelection(3, 'ArrowRight', sections, 4)).toBe(4);
    expect(nextSelection(3, 'End', sections, 4)).toBe(7);
  });
});

describe('positionInView past the end', () => {
  it('never counts past the view either', () => {
    expect(positionInView('recent', [], 50, 20)).toEqual({ index: 20, count: 20 });
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
