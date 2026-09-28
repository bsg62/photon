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

describe('writes from code', () => {
  it('maps a virtual target to its proportional DOM position and takes it on the write', () => {
    const map = mapped();
    expect(map.setVirtual(4_950)).toBe(1_000);
    map.wrote(1_000);
    expect(map.virtual).toBe(4_950);
    expect(map.shift).toBe(3_950);
  });
  it('clamps a target past the end', () => {
    const map = mapped();
    expect(map.setVirtual(20_000)).toBe(2_000);
    map.wrote(2_000);
    expect(map.virtual).toBe(9_900);
  });
  it('does not re-apply the scroll event its own write causes', () => {
    const map = mapped();
    map.wrote(map.setVirtual(4_950));
    map.onScroll(1_000);
    expect(map.virtual).toBe(4_950);
    map.onScroll(1_010);
    expect(map.virtual).toBe(4_960);
  });
  it('an echo within a pixel of the write is recognised', () => {
    const map = mapped();
    map.setVirtual(4_950);
    map.wrote(1_000.4);
    map.onScroll(1_000);
    expect(map.virtual).toBe(4_950);
  });
  it('under the cap hands the target to the browser unclamped', () => {
    const map = createScrollMap();
    map.resize(2_000, 100, 2_100);
    expect(map.setVirtual(5_000)).toBe(5_000);
    map.wrote(1_900);
    expect(map.virtual).toBe(1_900);
    expect(map.shift).toBe(0);
  });
  it('reads a DOM position ahead of its scroll event through the shift', () => {
    const map = mapped();
    map.wrote(map.setVirtual(4_950));
    expect(map.virtualAt(1_020)).toBe(4_970);
    const under = createScrollMap();
    under.resize(2_000, 100, 2_100);
    expect(under.virtualAt(1_020)).toBe(1_020);
  });
});

describe('re-anchoring', () => {
  it('puts the thumb back on settle, without moving the photos', () => {
    const map = mapped();
    map.press(true);
    map.onScroll(1_000); // virtual 4_950
    map.release();
    map.onScroll(1_100); // virtual 5_050, shift 3_950
    const w = map.settle();
    expect(w).toBeCloseTo(5_050 / 4.95, 6);
    map.wrote(w!);
    expect(map.virtual).toBe(5_050);
    expect(map.shift).toBeCloseTo(5_050 - 5_050 / 4.95, 6);
  });
  it('writes nothing on settle when the thumb is already right', () => {
    const map = mapped();
    map.press(true);
    map.onScroll(1_000);
    expect(map.settle()).toBeNull();
  });
  it('writes nothing on settle under the cap', () => {
    const map = createScrollMap();
    map.resize(2_000, 100, 2_100);
    map.onScroll(700);
    expect(map.settle()).toBeNull();
  });
  it('re-anchors at once when a step reaches the top of the DOM before the top of the library', () => {
    const map = mapped();
    map.press(true);
    map.onScroll(1_000); // virtual 4_950
    map.release();
    let w: number | null = null;
    for (let d = 850; w === null && d > -150; d -= 150) w = map.onScroll(Math.max(0, d));
    expect(map.virtual).toBe(3_950);
    expect(w).toBeCloseTo(3_950 / 4.95, 6);
  });
  it('re-anchors at once when a step reaches the bottom of the DOM before the end', () => {
    const map = mapped();
    map.press(true);
    map.onScroll(1_900); // virtual 9_405
    map.release();
    const w = map.onScroll(2_000); // virtual 9_505: the DOM is at its end, the library is not
    expect(w).toBeCloseTo(9_505 / 4.95, 6);
  });
  it('an overlay thumb drag read as relative settles onto the thumb', () => {
    const map = mapped();
    for (let d = 100; d <= 1_000; d += 100) map.onScroll(d); // no press: small steps, relative
    expect(map.virtual).toBe(1_000);
    const w = map.settle();
    expect(w).toBeCloseTo(1_000 / 4.95, 6);
  });
  it('a re-anchor keeps every screen position at the same virtual y', () => {
    const map = mapped();
    map.press(true);
    map.onScroll(1_000);
    map.release();
    map.onScroll(1_100);
    const before = map.virtualAt(1_100);
    const w = map.settle()!;
    map.wrote(w);
    expect(map.virtualAt(w)).toBeCloseTo(before, 6);
  });
  it('settle forgets an echo that never came', () => {
    const map = mapped();
    map.wrote(map.setVirtual(4_950)); // expected 1_000
    map.settle();
    map.onScroll(1_000.5);
    expect(map.virtual).toBeCloseTo(4_950.5, 6);
  });
});

describe('resizing', () => {
  it('a rebuild that stays past the cap keeps shift and writes nothing', () => {
    const map = mapped();
    map.press(true);
    map.onScroll(1_000);
    map.release();
    const shift = map.shift;
    expect(map.resize(12_000, 100, 2_100)).toBeNull();
    expect(map.shift).toBe(shift);
    expect(map.virtual).toBe(4_950);
  });
  it('entering the mapped range keeps the place the grid was at', () => {
    const map = createScrollMap();
    map.resize(2_000, 100, 2_100);
    map.onScroll(700);
    const w = map.resize(10_000, 100, 2_100);
    expect(w).toBeCloseTo(700 / 4.95, 6);
    map.wrote(w!);
    expect(map.virtual).toBe(700);
  });
  it('leaving it writes the virtual position itself', () => {
    const map = mapped();
    map.press(true);
    map.onScroll(400); // virtual 1_980
    map.release();
    expect(map.resize(2_050, 100, 2_100)).toBe(1_950); // clamped to the new end
    map.wrote(1_950);
    expect(map.shift).toBe(0);
    expect(map.virtual).toBe(1_950);
  });
  it('a new cap (the display scale changed) re-anchors', () => {
    const map = mapped();
    map.press(true);
    map.onScroll(1_000);
    map.release();
    const w = map.resize(10_000, 100, 1_100); // maxDom 1_000
    expect(w).toBeCloseTo((4_950 / 9_900) * 1_000, 6);
  });
  it('a library that shrinks under the place the grid was at is clamped and re-anchored', () => {
    const map = mapped();
    map.press(true);
    map.onScroll(2_000); // virtual 9_900
    map.release();
    const w = map.resize(6_000, 100, 2_100);
    expect(w).toBeCloseTo(2_000, 6);
    map.wrote(w!);
    expect(map.virtual).toBe(5_900);
  });
  it('a taller viewport that leaves the DOM position past the end re-anchors', () => {
    const map = mapped();
    map.press(true);
    map.onScroll(2_000);
    map.release();
    expect(map.resize(10_000, 300, 2_100)).toBeCloseTo(1_800, 6);
  });
  it('under the cap before and after writes nothing', () => {
    const map = createScrollMap();
    map.resize(2_000, 100, 2_100);
    map.onScroll(700);
    expect(map.resize(1_500, 100, 2_100)).toBeNull();
  });
});

describe('a press on the scrollbar', () => {
  it('release ends a press', () => {
    const map = mapped();
    map.press(true);
    map.onScroll(1_000);
    map.release();
    map.onScroll(1_050);
    expect(map.virtual).toBe(5_000);
  });
  it('a press elsewhere is not a thumb drag', () => {
    const map = mapped();
    map.press(false);
    map.onScroll(150);
    expect(map.virtual).toBe(150);
  });
});
