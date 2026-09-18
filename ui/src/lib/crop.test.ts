import { describe, expect, it } from 'vitest';
import { FULL, MIN_SIDE, dragRect, fitAspect, fractionRatio, fromWire, toWire, type Rect } from './crop';

const mid: Rect = { left: 0.25, top: 0.25, right: 0.75, bottom: 0.75 };

function expectRect(actual: Rect, expected: Rect) {
  for (const side of ['left', 'top', 'right', 'bottom'] as const) {
    expect(actual[side], side).toBeCloseTo(expected[side], 9);
  }
}

const shape = (r: Rect) => (r.right - r.left) / (r.bottom - r.top);
const inside = (r: Rect) => r.left >= -1e-12 && r.top >= -1e-12 && r.right <= 1 + 1e-12 && r.bottom <= 1 + 1e-12;

describe('dragRect, free', () => {
  it('moves only the sides the handle names', () => {
    expectRect(dragRect(mid, 'e', 0.1, 0.4, null), { ...mid, right: 0.85 });
    expectRect(dragRect(mid, 'nw', -0.1, 0.05, null), { ...mid, left: 0.15, top: 0.3 });
  });

  it('stops at the picture and at the minimum size', () => {
    expectRect(dragRect(mid, 'se', 9, 9, null), { ...mid, right: 1, bottom: 1 });
    expectRect(dragRect(mid, 'w', 9, 0, null), { ...mid, left: 0.75 - MIN_SIDE });
    expectRect(dragRect(mid, 'n', 0, -9, null), { ...mid, top: 0 });
  });

  it('moves the whole rectangle without resizing it, and keeps it on the picture', () => {
    expectRect(dragRect(mid, 'move', 0.1, -0.1, null), { left: 0.35, top: 0.15, right: 0.85, bottom: 0.65 });
    expectRect(dragRect(mid, 'move', 9, 9, null), { left: 0.5, top: 0.5, right: 1, bottom: 1 });
  });
});

describe('dragRect, locked', () => {
  // A locked drag starts from a rectangle that already has the ratio: choosing a preset
  // runs `fitAspect` first. 2:1 about the middle is 0.5 wide and 0.25 tall.
  const wide = fitAspect(mid, 2);

  it('keeps the ratio from a corner, anchored at the opposite corner', () => {
    const r = dragRect(wide, 'se', 0.1, 0, 2);
    expect(shape(r)).toBeCloseTo(2, 9);
    expect([r.left, r.top]).toEqual([0.25, 0.375]);
    expect(r.right).toBeCloseTo(0.85, 9);
  });

  it('follows the axis the pointer moved further along', () => {
    // Down by 0.1 asks for a height of 0.35, which at 2:1 is 0.7 wide: more than the 0.5
    // the pointer's x asks for, so the height leads.
    const r = dragRect(wide, 'se', 0, 0.1, 2);
    expect(shape(r)).toBeCloseTo(2, 9);
    expect(r.bottom).toBeCloseTo(0.725, 9);
    expect(r.right).toBeCloseTo(0.95, 9);
  });

  it('stops both dimensions when the picture stops one', () => {
    const r = dragRect(mid, 'ne', 9, -9, 1);
    expect(shape(r)).toBeCloseTo(1, 9);
    expect(inside(r)).toBe(true);
    expect([r.left, r.bottom]).toEqual([0.25, 0.75]);
    expect(r.top).toBeCloseTo(0, 9);
  });

  it('grows an edge about the middle of the other axis', () => {
    const r = dragRect(mid, 'e', 0.1, 0, 1);
    expect(shape(r)).toBeCloseTo(1, 9);
    expect(r.left).toBe(0.25);
    expect((r.top + r.bottom) / 2).toBeCloseTo(0.5, 9);
    const s = dragRect(mid, 's', 0, -9, 2);
    expect(shape(s)).toBeCloseTo(2, 9);
    expect(s.bottom - s.top).toBeGreaterThanOrEqual(MIN_SIDE - 1e-12);
    expect((s.left + s.right) / 2).toBeCloseTo(0.5, 9);
  });

  it('never leaves the picture, whatever the drag', () => {
    const starts: Rect[] = [mid, { left: 0, top: 0, right: 0.3, bottom: 0.9 }, { left: 0.6, top: 0.1, right: 1, bottom: 0.4 }];
    for (const start of starts) {
      for (const handle of ['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw'] as const) {
        for (const k of [0.5, 1, 2.5]) {
          for (const [dx, dy] of [[9, 9], [-9, -9], [9, -9], [0.01, -0.3]]) {
            const r = dragRect(fitAspect(start, k), handle, dx, dy, k);
            expect(inside(r), `${handle} k=${k} d=${dx},${dy}`).toBe(true);
            expect(shape(r)).toBeCloseTo(k, 6);
          }
        }
      }
    }
  });
});

describe('fractionRatio', () => {
  it('divides the picture out, so a pixel ratio becomes a ratio of fractions', () => {
    // A 3:2 photo cropped to 1:1 keeps two thirds of its width for all of its height.
    expect(fractionRatio(1, 1.5)).toBeCloseTo(1 / 1.5, 9);
    expect(fractionRatio('original', 1.5)).toBe(1);
    expect(fractionRatio(null, 1.5)).toBeNull();
  });

  it('turns a preset to face the way the picture does', () => {
    // 4:3 on a portrait photo means 3:4 pixels.
    expect(fractionRatio(4 / 3, 2 / 3)).toBeCloseTo(3 / 4 / (2 / 3), 9);
  });
});

describe('fitAspect', () => {
  it('shrinks one dimension about the centre', () => {
    expectRect(fitAspect(FULL, 0.5), { left: 0.25, top: 0, right: 0.75, bottom: 1 });
    expectRect(fitAspect(FULL, 2), { left: 0, top: 0.25, right: 1, bottom: 0.75 });
    expect(fitAspect(mid, null)).toBe(mid);
  });
});

describe('the wire form', () => {
  it('round-trips, and says "no crop" for the whole picture', () => {
    expect(toWire(FULL)).toBeNull();
    expect(toWire({ left: 0.5, top: 0, right: 1, bottom: 0.5 })).toEqual([32768, 0, 65535, 32768]);
    expectRect(fromWire([0, 0, 65535, 65535]), FULL);
    expect(fromWire(null)).toBe(FULL);
    const wire = toWire(mid)!;
    expect(toWire(fromWire(wire))).toEqual(wire);
  });

  it('cannot produce a coordinate outside the unit', () => {
    expect(toWire({ left: -0.2, top: 0.1, right: 1.4, bottom: 0.9 })).toEqual([0, 6554, 65535, 58982]);
  });
});
