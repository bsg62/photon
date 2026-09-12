import { describe, expect, it } from 'vitest';
import { clampPan, clampZoom, move, positionInFolder, wheelStep } from './nav';

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
