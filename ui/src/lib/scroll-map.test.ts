import { describe, expect, it } from 'vitest';
import { CAP_FALLBACK, capFrom, createScrollMap } from './scroll-map';

/** A map past the cap: total 10_000, viewport 100, domMax 2_100 - so maxDom 2_000 and
 *  maxVirtual 9_900, and a proportional DOM position d is virtual d × 4.95. */
function mapped() {
  const map = createScrollMap();
  const w = map.resize(10_000, 100, 2_100);
  expect(w).toBe(0); // entering the mapped range from the top anchors at 0
  map.wrote(0);
  return map;
}

describe('capFrom', () => {
  it('keeps 90% of what the engine allowed', () => {
    expect(capFrom(33_554_428)).toBe(30_198_985);
    expect(capFrom(16_777_214)).toBe(15_099_492);
  });
  it('refuses a reading from a window not laid out yet', () => {
    expect(capFrom(0)).toBeNull();
    expect(capFrom(999_999)).toBeNull();
    expect(capFrom(Number.NaN)).toBeNull();
  });
  it('takes an engine with no cap at its word', () => {
    expect(capFrom(1e9)).toBe(900_000_000);
  });
  it('has a fallback well under every engine and scale measured', () => {
    expect(CAP_FALLBACK).toBeLessThan(16_777_214 / 2);
  });
});

describe('under the cap', () => {
  it('is the identity: virtual is the DOM position and shift stays 0', () => {
    const map = createScrollMap();
    expect(map.resize(2_000, 100, 2_100)).toBeNull();
    expect(map.mapped).toBe(false);
    expect(map.domHeight).toBe(2_000);
    for (const top of [0, 700, 1_900, 3]) {
      expect(map.onScroll(top)).toBeNull();
      expect(map.virtual).toBe(top);
      expect(map.shift).toBe(0);
    }
  });
  it('ignores a scrollbar press', () => {
    const map = createScrollMap();
    map.resize(2_000, 100, 2_100);
    map.press(true);
    map.onScroll(1_500);
    expect(map.virtual).toBe(1_500);
  });
  it('follows the viewport past the canvas end (the Copies notice below it)', () => {
    const map = createScrollMap();
    map.resize(2_000, 100, 2_100);
    map.onScroll(1_950);
    expect(map.virtual).toBe(1_950);
  });
});

describe('past the cap', () => {
  it('holds the canvas at the cap', () => {
    const map = mapped();
    expect(map.mapped).toBe(true);
    expect(map.domHeight).toBe(2_100);
  });
  it('moves the virtual position 1:1 for a small step', () => {
    const map = mapped();
    map.onScroll(50);
    expect(map.virtual).toBe(50);
    map.onScroll(80);
    expect(map.virtual).toBe(80);
    expect(map.shift).toBe(0);
  });
  it('maps a scrollbar drag proportionally, and a small step after it keeps the shift', () => {
    const map = mapped();
    map.press(true);
    map.onScroll(1_000);
    expect(map.virtual).toBe(4_950);
    expect(map.shift).toBe(3_950);
    map.release();
    map.onScroll(1_010);
    expect(map.virtual).toBe(4_960);
    expect(map.shift).toBe(3_950);
  });
  it('maps the DOM ends to the virtual ends exactly', () => {
    const map = mapped();
    map.press(true);
    map.onScroll(2_000);
    expect(map.virtual).toBe(9_900);
    map.onScroll(0);
    expect(map.virtual).toBe(0);
  });
  it('reads a step of more than two viewports as a jump, and two viewports as a step', () => {
    const map = mapped();
    map.onScroll(200);
    expect(map.virtual).toBe(200);
    map.onScroll(401);
    expect(map.virtual).toBeCloseTo(401 * 4.95, 6);
  });
  it('a held press maps even a small step proportionally', () => {
    const map = mapped();
    map.press(true);
    map.onScroll(10);
    expect(map.virtual).toBeCloseTo(10 * 4.95, 6);
    map.release();
    map.onScroll(20);
    expect(map.virtual).toBeCloseTo(59.5, 6);
  });
});
