# Grid Canvas Cap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every photo in a library of any size stays reachable in the grid, by scrolling, End, a folder jump and the timeline, while libraries that fit under the browser's layout-height cap behave exactly as today.

**Architecture:** A pure module, `ui/src/lib/scroll-map.ts`, maps the grid's layout position ("virtual", what `scrollTop` state means everywhere today) onto a DOM scroll range capped below the engine's measured limit. Relative input (wheel, keys, touch, band autoscroll) moves the virtual position 1:1; a scrollbar drag or a jump of more than two viewports maps proportionally; the DOM position is re-anchored to the proportional one when scrolling goes still. `Grid.svelte` measures the cap, routes every DOM scroll read and write through the map, and draws mounted rows at `row.top − shift`. A headless-Chromium xtask, `scroll-probe`, proves the end of a 300k-photo library is reachable.

**Tech Stack:** Svelte 5 runes, TypeScript, vitest (`environment: 'node'`), Rust xtask driving headless Chromium.

**Spec:** `docs/superpowers/specs/2026-09-28-photon-grid-canvas-cap-design.md`

## Global Constraints

- Under the cap nothing changes: the mapping is the identity, `shift` is 0, and no scroll position is written that is not written today.
- Past the cap, the wheel stays 1:1 and the scrollbar thumb is approximate; the timeline stays exact.
- The cap is measured at runtime: `domMax = floor(0.9 × measured)`; a reading below 1,000,000 px (or not a number) falls back to 8,000,000 and is measured again on the next resize.
- Proportional when a press on the viewport's scrollbar is held, or when one scroll event moves the DOM more than two viewports.
- Rows are drawn at `style:top = row.top − shift`, never through a `translateY` wrapper.
- Never launch the GUI to verify (CLAUDE.md). Verification is vitest, `svelte-check`, the `scroll-probe` xtask and the README smoke checklist.
- Every new test is shown to fail with its rule reverted by an exact replacement (CLAUDE.md, "A new test must be demonstrated to fail").
- UI gate: `npm run check` (0 errors AND 0 warnings) and `npm test`. Rust gate for the xtask: `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.

## Review Focus

1. **A thumb drag on an overlay scrollbar** (GTK, macOS): no press lands in the gutter, so small drag steps read as relative. Expected: the content follows, and the thumb settles onto the true position on the pause. Pinned in Task 3 (`an overlay thumb drag read as relative settles onto the thumb`).
2. **A press on the scrollbar whose release never arrives** (the native scrollbar can swallow `pointerup`): expected, the next wheel scroll is relative, not proportional. Pinned in Task 2 (`release ends a press`); the `buttons === 0` wiring in Task 6.
3. **A scan rebuilding the grid during a flick** in a library past the cap: expected, no scroll write, the flick continues. Pinned in Task 4 (`a rebuild that stays past the cap keeps shift and writes nothing`).
4. **A rubber band autoscrolling across a re-anchor:** expected, the band's canvas coordinates are the same before and after. Pinned in Task 3 (`a re-anchor keeps every screen position at the same virtual y`).
5. **A fractional `scrollTop` read-back** (Windows at 125%/150%): expected, the echo of photon's own write is still recognised. Pinned in Task 2 (`an echo within a pixel of the write is recognised`).

---

## File Structure

- **Create `ui/src/lib/scroll-map.ts`**: the cap reading (`capFrom`) and `createScrollMap()`. Pure, no DOM, no runes.
- **Create `ui/src/lib/scroll-map.test.ts`**: vitest, server project (no reactivity needed).
- **Modify `ui/src/components/Grid.svelte`**: cap measurement, the map's wiring, `shift` in `style:top`, `.canvas` height.
- **Modify `crates/xtask/screenshots/mock.js`**: a `?huge=<photos>&folders=<n>` library and `?do=probe-end` / `?do=probe-bottom` actions that write a result into `document.title`.
- **Create `crates/xtask/src/scroll_probe.rs`**: the `scroll-probe` subcommand.
- **Modify `crates/xtask/src/screenshots.rs`**: extract `build_and_serve` from `run`, shared with `scroll-probe`.
- **Modify `crates/xtask/src/main.rs`**: dispatch `scroll-probe`.
- **Modify `README.md`** (smoke checklist), **`CLAUDE.md`** (the grid's scroll position is virtual), **`docs/superpowers/plans/2026-09-28-performance-audit-open-items.md`** (item 6 done).

Test numbers used throughout Tasks 1-4: `total = 10_000`, `viewport = 100`, `domMax = 2_100`, so `domHeight = 2_100`, `maxDom = 2_000`, `maxVirtual = 9_900`, and a proportional DOM position `d` maps to `d × 4.95`.

---

### Task 1: The cap reading and the map's core (identity, relative, proportional)

**Files:**
- Create: `ui/src/lib/scroll-map.ts`
- Test: `ui/src/lib/scroll-map.test.ts`

**Interfaces:**
- Produces:
  - `export const CAP_MARGIN = 0.9`, `CAP_FALLBACK = 8_000_000`, `CAP_MIN_READING = 1_000_000`, `PROBE_HEIGHT = 1e9`, `JUMP_VIEWPORTS = 2`
  - `export function capFrom(measured: number): number | null`
  - `export function createScrollMap(): ScrollMap` with (this task) `resize(total: number, viewport: number, domMax: number): number | null`, `onScroll(domTop: number): number | null`, `press(onScrollbar: boolean): void`, `release(): void`, getters `virtual`, `shift`, `domHeight`, `mapped`
  - `export type ScrollMap = ReturnType<typeof createScrollMap>`
  - Every method returning `number | null` returns a DOM `scrollTop` the caller must write (then report with `wrote`, Task 2), or `null` for none.

- [ ] **Step 1: Write the failing tests**

```ts
// ui/src/lib/scroll-map.test.ts
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
});
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `npm test -w ui -- src/lib/scroll-map.test.ts`
Expected: FAIL, `Failed to resolve import "./scroll-map"`.

- [ ] **Step 3: Write the module**

```ts
// ui/src/lib/scroll-map.ts
/** Keeping every photo reachable when the grid is taller than the browser will lay out.
 *
 *  Engines cap a box at (2^31 - 1) / 64 px - lower in CSS px on a scaled Windows display,
 *  where Chromium applies it in device pixels - and a canvas past it stops growing: every
 *  row beyond is laid out on the cap and `scrollTop` stops there. So the grid keeps two
 *  positions. *Virtual* is the layout's own (`buildRows`), what every consumer of the
 *  grid's `scrollTop` state reads. The *DOM* position is the viewport's `scrollTop` over a
 *  canvas held at `domHeight`. They differ by `shift`, and the mounted rows are drawn at
 *  `row.top - shift`.
 *
 *  Under the cap the two are the same and `shift` is 0 - the map is the identity, and a
 *  library that fits behaves exactly as it did before the map existed.
 *
 *  Past it, a DOM range shorter than the layout cannot be both 1:1 for the wheel and
 *  proportional for the thumb. The wheel wins: a small step moves the virtual position by
 *  the same amount (`shift` unchanged), and only a held press on the scrollbar, or a jump
 *  too big for any wheel, maps proportionally. The thumb drifts while scrolling and is put
 *  back where it belongs when scrolling stops (`settle`).
 *
 *  Every method that returns `number | null` returns a DOM `scrollTop` for the caller to
 *  write - and then report with `wrote` - or null for none. */

/** The share of the measured cap the canvas may use: room for the rest of the viewport's
 *  content (the Copies notice below the canvas) and for an engine rounding differently. */
export const CAP_MARGIN = 0.9;
/** Used until a trustworthy reading exists: under the cap of every engine measured, at
 *  every display scale up to 4x (Chromium at 2x: 16,777,214). */
export const CAP_FALLBACK = 8_000_000;
/** A probe reading below this came from a window not yet laid out, not from an engine. */
export const CAP_MIN_READING = 1_000_000;
/** How tall the probe asks to be: far past every engine's cap. */
export const PROBE_HEIGHT = 1e9;
/** A single scroll event moving the DOM more than this many viewports is not a wheel: a
 *  track click, or a thumb drag on an overlay scrollbar no press could be seen on. */
export const JUMP_VIEWPORTS = 2;

/** The canvas height to allow, from the height a `PROBE_HEIGHT` box was given; null when
 *  the reading cannot be trusted (see `CAP_MIN_READING`). */
export function capFrom(measured: number): number | null {
  if (!(measured >= CAP_MIN_READING)) return null;
  return Math.floor(measured * CAP_MARGIN);
}

export function createScrollMap() {
  let total = 0;
  let viewport = 0;
  let domMax = Number.POSITIVE_INFINITY;
  let virtual = 0;
  let shift = 0;
  let lastDomTop = 0;
  let onScrollbar = false;
  /** The virtual position a write the caller is about to make will take the grid to. */
  let pending: number | null = null;

  const isMapped = () => total > domMax;
  const domHeight = () => Math.min(total, domMax);
  const maxVirtual = () => Math.max(0, total - viewport);
  const maxDom = () => Math.max(0, domHeight() - viewport);
  const clampVirtual = (v: number) => Math.min(maxVirtual(), Math.max(0, v));

  /** The virtual position a DOM position stands for, proportionally; exact at both ends so
   *  a thumb dragged to the bottom lands on the last row. */
  function fromDom(d: number): number {
    const md = maxDom();
    if (md === 0 || d <= 0) return 0;
    if (d >= md) return maxVirtual();
    return (d / md) * maxVirtual();
  }

  /** The DOM position that stands for `v`, proportionally. */
  function toDom(v: number): number {
    const mv = maxVirtual();
    return mv === 0 ? 0 : (v / mv) * maxDom();
  }

  /** Aims the grid at virtual `v`: the DOM position to write for it, past the cap. */
  function target(v: number): number {
    pending = clampVirtual(v);
    return toDom(pending);
  }

  return {
    get virtual() {
      return virtual;
    },
    get shift() {
      return shift;
    },
    get domHeight() {
      return domHeight();
    },
    get mapped() {
      return isMapped();
    },

    /** A press on the viewport: `scrollbar` when it landed on the scrollbar itself. */
    press(scrollbar: boolean): void {
      onScrollbar = scrollbar;
    },
    /** The press is over - or was lost, which the grid infers from a pointer moving with no
     *  button held. */
    release(): void {
      onScrollbar = false;
    },

    /** The layout's height, the viewport's, and the cap. (Completed in Task 4.) */
    resize(nextTotal: number, nextViewport: number, nextDomMax: number): number | null {
      const wasMapped = isMapped();
      total = nextTotal;
      viewport = nextViewport;
      domMax = nextDomMax;
      if (!isMapped()) return null;
      if (!wasMapped) return target(virtual);
      virtual = clampVirtual(virtual);
      shift = virtual - lastDomTop;
      return null;
    },

    /** What the browser took from a write the caller just made, read back from the
     *  viewport: it may have rounded or clamped the value written. */
    wrote(domTop: number): void {
      lastDomTop = domTop;
      if (!isMapped()) {
        virtual = domTop;
        shift = 0;
        pending = null;
        return;
      }
      if (pending !== null) virtual = pending;
      pending = null;
      shift = virtual - domTop;
    },

    /** A scroll event, with the DOM position it left. */
    onScroll(domTop: number): number | null {
      if (!isMapped()) {
        // Not clamped to the canvas: the Copies notice below it lets the viewport scroll a
        // little past the layout's end, and the browser's position is the truth there.
        virtual = domTop;
        shift = 0;
        lastDomTop = domTop;
        return null;
      }
      const moved = domTop - lastDomTop;
      lastDomTop = domTop;
      if (onScrollbar || Math.abs(moved) > JUMP_VIEWPORTS * viewport) {
        virtual = fromDom(domTop);
      } else {
        virtual = clampVirtual(virtual + moved);
      }
      shift = virtual - domTop;
      return null;
    },
  };
}

export type ScrollMap = ReturnType<typeof createScrollMap>;
```

Also add this test to the `under the cap` block in Step 1's file (it pins the identity branch: the mapped path would clamp):

```ts
  it('follows the viewport past the canvas end (the Copies notice below it)', () => {
    const map = createScrollMap();
    map.resize(2_000, 100, 2_100);
    map.onScroll(1_950);
    expect(map.virtual).toBe(1_950);
  });
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `npm test -w ui -- src/lib/scroll-map.test.ts`
Expected: PASS.

- [ ] **Step 5: Revert probes**

Each is an exact replacement in `scroll-map.ts`, run, then restored (`git checkout ui/src/lib/scroll-map.ts` is NOT the restore - the file is uncommitted; keep a copy in the scratchpad and copy it back):
- `if (!(measured >= CAP_MIN_READING)) return null;` → `if (measured <= 0) return null;` - expect `refuses a reading…` to fail.
- `if (onScrollbar || Math.abs(moved) > JUMP_VIEWPORTS * viewport) {` → `if (Math.abs(moved) > JUMP_VIEWPORTS * viewport) {` - expect `maps a scrollbar drag…` to fail.
- `if (onScrollbar || Math.abs(moved) > JUMP_VIEWPORTS * viewport) {` → `if (onScrollbar) {` - expect `reads a step of more than two viewports…` to fail.
- `if (d >= md) return maxVirtual();` → `` (deleted) - expect `maps the DOM ends…` to fail only if the proportional formula misses the end; if it passes, record it in the commit message as a guard for rounding, not a discriminating rule.
- `if (!isMapped()) {` (in `onScroll`) → `if (false) {` - expect `follows the viewport past the canvas end…` to fail (the mapped path clamps to `total − viewport`). `is the identity…` alone would pass: at a ratio of 1 the mapped path computes the same positions, which is why the Copies-notice case exists.

- [ ] **Step 6: Gate and commit**

Run: `npm run check && npm test`
Expected: 0 errors, 0 warnings; all tests pass.

```bash
git add ui/src/lib/scroll-map.ts ui/src/lib/scroll-map.test.ts
git commit -m "feat(grid): a scroll map from the layout's position to a capped DOM range

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Writes from code, their echo, and the scrollbar press

**Files:**
- Modify: `ui/src/lib/scroll-map.ts`
- Test: `ui/src/lib/scroll-map.test.ts`

**Interfaces:**
- Consumes: Task 1's `createScrollMap`.
- Produces:
  - `setVirtual(v: number): number` - the DOM `scrollTop` to write for virtual position `v`. Under the cap `v` itself, unclamped (the browser clamps, as today; the Copies notice below the canvas makes the scroll range longer than the canvas). Past it, `v` clamped to `[0, total − viewport]` and mapped proportionally.
  - `wrote(domTop: number): void` - the value read back from `viewport.scrollTop` right after writing. Past the cap it takes the virtual position `setVirtual` (or a re-anchor, Task 3) chose, sets `shift`, and remembers `domTop` as the echo to expect.
  - `virtualAt(domTop: number): number` - the virtual position for a DOM position read *now*, before its scroll event: `domTop` under the cap, `clamp(domTop + shift)` past it.

- [ ] **Step 1: Write the failing tests**

Append to `ui/src/lib/scroll-map.test.ts`:

```ts
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
```

- [ ] **Step 2: Run to see them fail**

Run: `npm test -w ui -- src/lib/scroll-map.test.ts`
Expected: FAIL, `map.setVirtual is not a function`.

- [ ] **Step 3: Implement**

In `createScrollMap`, add beside `pending`:

```ts
  /** The DOM position a write from code left, whose scroll event is still to come. */
  let expected: number | null = null;
```

Add to the returned object:

```ts
    /** The DOM position to write for virtual position `v`. Under the cap `v` itself,
     *  unclamped: the browser clamps it, as it always has. */
    setVirtual(v: number): number {
      if (!isMapped()) {
        pending = null;
        return v;
      }
      return target(v);
    },

    /** The virtual position for a DOM position read now, ahead of its scroll event. */
    virtualAt(domTop: number): number {
      return isMapped() ? clampVirtual(domTop + shift) : domTop;
    },
```

In `wrote`, set and clear the expected echo: add `expected = null;` to the `!isMapped()` branch, and `expected = domTop;` after `shift = virtual - domTop;` at its end.

In `onScroll`, right after the `!isMapped()` branch:

```ts
      // The event photon's own write caused: already applied by `wrote`. Within a pixel, not
      // equal - a scaled display reads a fractional position back and reports another.
      if (expected !== null && Math.abs(domTop - expected) < 1) {
        expected = null;
        lastDomTop = domTop;
        return null;
      }
      expected = null;
```

- [ ] **Step 4: Run to see them pass**

Run: `npm test -w ui -- src/lib/scroll-map.test.ts`
Expected: PASS.

- [ ] **Step 5: Revert probes**

- `if (expected !== null && Math.abs(domTop - expected) < 1) {` → `if (expected !== null && domTop === expected) {` - expect `an echo within a pixel…` to fail.
- `if (expected !== null && Math.abs(domTop - expected) < 1) {` → `if (false) {` - expect `does not re-apply…` to fail (the echo moves virtual by `1_000 − 1_000 = 0`… it does not move: if this probe passes, the echo is harmless on its own; then write the case that differs - a write whose read-back differs from `lastDomTop` before it - and record it).
- In `setVirtual`: `return v;` → `return Math.min(v, 1_900);` - expect `under the cap hands the target…` to fail.
- `release(): void { onScrollbar = false; }` → `release(): void {}` - expect `release ends a press` to fail.

- [ ] **Step 6: Gate and commit**

Run: `npm run check && npm test`

```bash
git add ui/src/lib/scroll-map.ts ui/src/lib/scroll-map.test.ts
git commit -m "feat(grid): writes from code through the scroll map, and their echo

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Re-anchoring (on settle, and at a DOM edge)

**Files:**
- Modify: `ui/src/lib/scroll-map.ts`
- Test: `ui/src/lib/scroll-map.test.ts`

**Interfaces:**
- Consumes: Task 2's `target`, `toDom`, `wrote`.
- Produces:
  - `settle(): number | null` - called when scrolling has gone still. Past the cap, the DOM position that puts the thumb back on the virtual position (a write; nothing on screen moves), or `null` when it is already there. Under the cap always `null`. Always forgets an echo still expected.
  - `onScroll` returns a write when a relative step reaches a DOM end while the virtual position is not at the matching end (or the reverse).

- [ ] **Step 1: Write the failing tests**

```ts
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
```

- [ ] **Step 2: Run to see them fail**

Run: `npm test -w ui -- src/lib/scroll-map.test.ts`
Expected: FAIL, `map.settle is not a function`, and the edge tests fail with `expected null`.

- [ ] **Step 3: Implement**

Add to the returned object:

```ts
    /** Scrolling has gone still: the DOM position that puts the thumb back where the virtual
     *  position is, or null when it is already there. Nothing on screen moves - `virtual`
     *  is unchanged and `shift` absorbs the difference. */
    settle(): number | null {
      expected = null;
      if (!isMapped()) return null;
      return Math.abs(toDom(virtual) - lastDomTop) < 1 ? null : target(virtual);
    },
```

At the end of `onScroll`'s relative branch, replace `shift = virtual - domTop; return null;` with:

```ts
      shift = virtual - domTop;
      // An end of one range without the other: relative steps have walked the DOM into a
      // wall the library has not reached (or the reverse), and the next step would go
      // nowhere. Re-anchored now rather than on settle - it can cut a flick short, but only
      // after millions of px without a pause.
      const md = maxDom();
      const mv = maxVirtual();
      if (domTop < 1 !== virtual <= 0 || domTop > md - 1 !== virtual >= mv) return target(virtual);
      return null;
```

(The proportional branch keeps `shift = virtual - domTop; return null;`: it maps ends to ends.)

- [ ] **Step 4: Run to see them pass**

Run: `npm test -w ui -- src/lib/scroll-map.test.ts`
Expected: PASS.

- [ ] **Step 5: Revert probes**

- `return Math.abs(toDom(virtual) - lastDomTop) < 1 ? null : target(virtual);` → `return null;` - expect the two settle-writes tests to fail.
- `expected = null;` (first line of `settle`) → `` - expect `settle forgets an echo…` to fail.
- `if (domTop < 1 !== virtual <= 0 || domTop > md - 1 !== virtual >= mv) return target(virtual);` → `if (domTop > md - 1 !== virtual >= mv) return target(virtual);` - expect the top-edge test to fail.
- same line → `if (domTop < 1 !== virtual <= 0) return target(virtual);` - expect the bottom-edge test to fail.

- [ ] **Step 6: Gate and commit**

Run: `npm run check && npm test`

```bash
git add ui/src/lib/scroll-map.ts ui/src/lib/scroll-map.test.ts
git commit -m "feat(grid): re-anchor the thumb on settle and at a DOM edge

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Resizes - a rebuild, a crossing, a new cap

**Files:**
- Modify: `ui/src/lib/scroll-map.ts`
- Test: `ui/src/lib/scroll-map.test.ts`

**Interfaces:**
- Consumes: Tasks 1-3.
- Produces: `resize(total, viewport, domMax): number | null` with these rules:
  - under the cap before and after: `null` (the browser's own clamp and scroll event keep `virtual`);
  - past the cap before and after, same `domHeight`: `virtual` clamped, `shift` kept; a write only when the clamp moved `virtual` or the DOM position fell outside `[0, maxDom]`;
  - entering the mapped range, leaving it, or a new `domHeight` (a new cap): re-anchor - `target(virtual)` past the cap; leaving, the write is `virtual` itself (the canvas holds the whole layout again).

- [ ] **Step 1: Write the failing tests**

```ts
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
```

- [ ] **Step 2: Run to see them fail**

Run: `npm test -w ui -- src/lib/scroll-map.test.ts`
Expected: FAIL on the leaving, new-cap, shrink and taller-viewport cases.

- [ ] **Step 3: Implement**

Replace `resize` with:

```ts
    /** The layout's height, the viewport's, and the cap - on every rebuild, resize, tile
     *  size change and new display scale. Called after the canvas has its new height: a
     *  write into a canvas not yet grown is clamped away. */
    resize(nextTotal: number, nextViewport: number, nextDomMax: number): number | null {
      const wasMapped = isMapped();
      const wasHeight = domHeight();
      total = nextTotal;
      viewport = nextViewport;
      domMax = nextDomMax;
      if (!wasMapped && !isMapped()) return null;
      if (!isMapped()) {
        // The canvas holds the whole layout again: the place to be is `virtual` itself.
        virtual = clampVirtual(virtual);
        pending = null;
        return virtual;
      }
      const clamped = clampVirtual(virtual);
      // Staying past the cap at the same height is the common case - a scan adding photos
      // during a flick - and writes nothing, so the flick carries on.
      if (wasMapped && domHeight() === wasHeight && clamped === virtual && lastDomTop >= 0 && lastDomTop <= maxDom()) {
        return null;
      }
      virtual = clamped;
      return target(virtual);
    },
```

`wrote` already handles leaving (its `!isMapped()` branch).

- [ ] **Step 4: Run to see them pass**

Run: `npm test -w ui -- src/lib/scroll-map.test.ts`
Expected: PASS.

- [ ] **Step 5: Revert probes**

- `if (wasMapped && domHeight() === wasHeight && clamped === virtual && lastDomTop >= 0 && lastDomTop <= maxDom()) {` → `if (wasMapped) {` - expect new-cap, shrink and taller-viewport to fail.
- same → `if (false) {` - expect `a rebuild that stays past the cap…` to fail (it would write).
- `virtual = clampVirtual(virtual);` (leaving branch) → `` - expect `leaving it writes…` to fail.

- [ ] **Step 6: Gate and commit**

Run: `npm run check && npm test`

```bash
git add ui/src/lib/scroll-map.ts ui/src/lib/scroll-map.test.ts
git commit -m "feat(grid): the scroll map across rebuilds, resizes and a new cap

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: `scroll-probe`: prove the end is unreachable today

This task lands before the wiring on purpose: it must fail against the current `Grid.svelte`.

**Files:**
- Modify: `crates/xtask/screenshots/mock.js`
- Create: `crates/xtask/src/scroll_probe.rs`
- Modify: `crates/xtask/src/screenshots.rs` (extract `build_and_serve`)
- Modify: `crates/xtask/src/main.rs`

**Interfaces:**
- Produces: `cargo run -p xtask -- scroll-probe [--no-build]`, exit 0 when every case passes. Page result contract: `document.title = 'PROBE ' + JSON.stringify({ last, cols, canvas, scrollHeight })`, where `last` is the highest grid offset with a mounted thumbnail (`img` whose `src` matches `/thumb\/(\d+)\/grid\//`, offset = id − 1 in the mock), `cols` the most tiles in one `.row`, `canvas` the `.canvas` element's `getBoundingClientRect().height`, `scrollHeight` the viewport's.

- [ ] **Step 1: The mock's huge library**

In `mock.js`, replace the fixed `sections`, `len` and `entry` with a generator used when `?huge=` is given, keeping the current values otherwise:

```js
  const HUGE = Number(P.get('huge')) || 0;
  const HUGE_FOLDERS = Number(P.get('folders')) || 1;
  if (HUGE > 0) {
    // A library past every engine's layout cap (`scroll-probe`): HUGE photos in HUGE_FOLDERS
    // folders of equal size, newest first, one folder a day.
    const per = Math.ceil(HUGE / HUGE_FOLDERS);
    folders.length = 0;
    sections.length = 0;
    for (let f = 0; f < HUGE_FOLDERS; f++) {
      const id = 100 + f;
      folders.push({ id, watchedId: 1, parentId: null, path: `/p/${f}`, name: `Folder ${f}`, hidden: false, alias: null });
      sections.push({ folderId: id, offset: f * per, count: Math.min(per, HUGE - f * per), takenAtMin: day(2026, 1, 1) - f * 86_400 });
    }
    len = HUGE;
  }
```

For this, `folders`, `sections` stay `const` arrays (mutated in place) and `len` becomes `let`. Make `entry` find its section by binary search, since 20,000 sections times a 1,000-row page is too slow as a linear scan:

```js
  function sectionOf(i) {
    let lo = 0;
    let hi = sections.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (sections[mid].offset <= i) lo = mid;
      else hi = mid - 1;
    }
    return sections[lo];
  }
```

and use `const section = sectionOf(i);` in `entry`.

- [ ] **Step 2: The probe actions**

Add to `actions`:

```js
    // `scroll-probe`: End through the grid's own key handling (a write from code), then read
    // what got mounted.
    'probe-end': () => {
      document.querySelector('.viewport')?.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }));
      later(3000, probeReport);
    },
    // A scrollbar-sized jump straight to the bottom of the DOM range.
    'probe-bottom': () => {
      const v = document.querySelector('.viewport');
      if (v) v.scrollTop = v.scrollHeight;
      later(3000, probeReport);
    },
```

and, above `actions`:

```js
  function probeReport() {
    const v = document.querySelector('.viewport');
    const ids = [...document.querySelectorAll('.canvas img')]
      .map((img) => /thumb\/(\d+)\/grid\//.exec(img.getAttribute('src') ?? '')?.[1])
      .filter(Boolean)
      .map(Number);
    const cols = Math.max(0, ...[...document.querySelectorAll('.canvas .row')].map((r) => r.children.length));
    document.title =
      'PROBE ' +
      JSON.stringify({
        last: ids.length ? Math.max(...ids) - 1 : -1,
        cols,
        canvas: document.querySelector('.canvas')?.getBoundingClientRect().height ?? -1,
        scrollHeight: v?.scrollHeight ?? -1,
      });
  }
```

If a tile's thumbnail is still deferred when the report runs (`last` is −1 or short by a row), raise the `later(3000, …)` delay; do not read ids from anything but mounted thumbnails - an entry fetched but not drawn proves nothing about reachability.

- [ ] **Step 3: The xtask**

Create `crates/xtask/src/scroll_probe.rs`:

```rust
//! `cargo run -p xtask -- scroll-probe`: whether the end of a library taller than the
//! browser's layout cap can be reached in the grid. Serves the built UI with `mock.js`
//! answering a 300,000-photo library, drives it in headless Chromium, and reads the result
//! the page writes into its title. Not in CI: it needs Chromium, like `screenshots`.

use crate::screenshots::build_and_serve;
use std::path::Path;
use std::process::{Command, ExitCode};

struct Case {
    name: &'static str,
    query: &'static str,
    window: &'static str,
    scale: Option<&'static str>,
    /// What the page must report.
    check: fn(&serde_json::Value) -> Result<(), String>,
}

fn last_is(v: &serde_json::Value, want: i64) -> Result<(), String> {
    match v["last"].as_i64() {
        Some(last) if last == want => Ok(()),
        other => Err(format!("last mounted offset {other:?}, want {want}")),
    }
}

fn one_column(v: &serde_json::Value) -> Result<(), String> {
    match v["cols"].as_i64() {
        Some(1) => Ok(()),
        other => Err(format!("{other:?} columns: the case is meant to have one")),
    }
}

const CASES: &[Case] = &[
    Case {
        name: "end, 1 column",
        query: "huge=300000&folders=20000&tile=large&do=probe-end",
        window: "660,800",
        scale: None,
        check: |v| one_column(v).and_then(|()| last_is(v, 299_999)),
    },
    Case {
        name: "scrollbar to the bottom, 1 column",
        query: "huge=300000&folders=20000&tile=large&do=probe-bottom",
        window: "660,800",
        scale: None,
        check: |v| one_column(v).and_then(|()| last_is(v, 299_999)),
    },
    Case {
        name: "end, 1 column, display scale 2",
        query: "huge=300000&folders=20000&tile=large&do=probe-end",
        window: "660,800",
        scale: Some("2"),
        check: |v| {
            let canvas = v["canvas"].as_f64().unwrap_or(-1.0);
            if canvas > 16_777_214.0 {
                return Err(format!("canvas {canvas} px is past Chromium's cap at scale 2"));
            }
            last_is(v, 299_999)
        },
    },
    Case {
        name: "a small library, scrollbar to the bottom",
        query: "do=probe-bottom",
        window: "1280,800",
        scale: None,
        check: |v| last_is(v, 90),
    },
];
```


The runner, reusing `screenshots`' server loop - read `screenshots::run` from `let listener = …` onwards and mirror it (bind loopback port 0, spawn a thread that calls `serve` for each accepted stream with `ui/dist`), then for each case:

```rust
fn run_case(chromium: &Path, port: u16, case: &Case) -> Result<(), String> {
    let mut args = vec![
        "--headless".to_owned(),
        "--disable-gpu".to_owned(),
        format!("--window-size={}", case.window),
        "--virtual-time-budget=20000".to_owned(),
        "--user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) photon-screenshots".to_owned(),
        format!("--host-resolver-rules=MAP photon.localhost 127.0.0.1:{port}"),
        "--dump-dom".to_owned(),
    ];
    if let Some(scale) = case.scale {
        args.push(format!("--force-device-scale-factor={scale}"));
    }
    args.push(format!("http://127.0.0.1:{port}/?theme=light&{}", case.query));
    let out = Command::new(chromium)
        .args(&args)
        .output()
        .map_err(|e| format!("cannot start {}: {e}", chromium.display()))?;
    let dom = String::from_utf8_lossy(&out.stdout);
    let title = dom
        .split("<title>PROBE ")
        .nth(1)
        .and_then(|rest| rest.split("</title>").next())
        .ok_or("the page wrote no PROBE title: the action did not run")?;
    let value: serde_json::Value =
        serde_json::from_str(&title.replace("&quot;", "\"")).map_err(|e| format!("{e}: {title}"))?;
    (case.check)(&value).map_err(|why| format!("{why} ({value})"))
}

pub fn run(root: &Path, args: &[String]) -> ExitCode {
    let (chromium, port) = match build_and_serve(root, args) {
        Ok(served) => served,
        Err(code) => return code,
    };
    let mut failed = 0;
    for case in CASES {
        match run_case(&chromium, port, case) {
            Ok(()) => println!("ok    {}", case.name),
            Err(why) => {
                failed += 1;
                println!("FAIL  {}: {why}", case.name);
            }
        }
    }
    if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
```

`build_and_serve` comes out of `screenshots::run`: move its body from finding Chromium through the `--no-build` handling, the `ui/dist` path, binding the loopback listener on port 0 and spawning the thread that calls `serve` for each stream, into `pub(crate) fn build_and_serve(root: &Path, args: &[String]) -> Result<(PathBuf, u16), ExitCode>` in `screenshots.rs` (Chromium's path and the port; each early `return ExitCode::FAILURE` becomes `return Err(ExitCode::FAILURE)`), and have `screenshots::run` call it. `screenshots`' own output directory handling stays in `screenshots::run`. Import it in `scroll_probe.rs` alongside `serve`/`flag` only if used; `find_chromium` is then called inside `build_and_serve`.

In `crates/xtask/src/main.rs`: add `mod scroll_probe;`, a `Some("scroll-probe") => scroll_probe::run(&repo_root(), &args),` arm, the usage line `//!   cargo run -p xtask -- scroll-probe [--no-build]`, and `scroll-probe` in the unknown-command message.

- [ ] **Step 4: Run it against today's grid and see it fail**

Run: `cargo run -p xtask -- scroll-probe`
Expected: the small-library case `ok`; the three huge cases `FAIL` with `last mounted offset Some(142…)` (about 142,341 at scale 1; lower at scale 2) - the bug, measured. If the small case fails, or `cols` is not 1, fix the probe, not the grid.

- [ ] **Step 5: Rust gate and commit**

Run the Rust gate (Global Constraints). The screenshots test that reads `api.ts` against `mock.js`'s `canned`/`SILENT` lists must still pass.

```bash
git add crates/xtask
git commit -m "test(xtask): scroll-probe, the end of a library past the layout cap

Fails today: End reaches offset ~142k of 300k at one column.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: Wire the map into `Grid.svelte`

**Files:**
- Modify: `ui/src/components/Grid.svelte`

**Interfaces:**
- Consumes: `createScrollMap`, `capFrom`, `CAP_FALLBACK`, `PROBE_HEIGHT` from `../lib/scroll-map`.
- Produces: nothing new for other files; `scrollToOffset`'s signature is unchanged (App calls it).

The component has no test harness (CLAUDE.md); the proof is `svelte-check`, the unchanged vitest suite, and Task 5's probe turning green.

- [ ] **Step 1: State and helpers**

Add to the imports: `import { CAP_FALLBACK, capFrom, createScrollMap, PROBE_HEIGHT } from '../lib/scroll-map';`

Below `let scrollTop = $state(0);`:

```ts
  /** `scrollTop` is the layout's position; the viewport's own `scrollTop` is the DOM's, and
   *  the two differ by `shift` in a library taller than the browser will lay out. Every
   *  read and write of the viewport's position goes through `map` (`scroll-map.ts`). */
  const map = createScrollMap();
  let shift = $state(0);
  /** The tallest canvas this engine, at this display scale, lays out; see `measureCap`. */
  let domMax = $state(Number.POSITIVE_INFINITY);
  let capTrusted = false;
```

Below `const total = $derived(totalHeight(rows));`:

```ts
  const domHeight = $derived(Math.min(total, domMax));
```

Near `scrollToOffset`, add:

```ts
  /** Writes a DOM position the map asked for and tells it what the browser took. The
   *  virtual position is published at once only past the cap: under it, the scroll event
   *  does that, as it always has. */
  function writeDom(domTop: number) {
    viewport.scrollTop = domTop;
    map.wrote(viewport.scrollTop);
    shift = map.shift;
    if (map.mapped) scrollTop = map.virtual;
  }

  function scrollToVirtual(v: number) {
    writeDom(map.setVirtual(v));
  }

  /** How tall a box this engine will lay out, in CSS px, at this display scale. */
  function measureCap() {
    const probe = document.createElement('div');
    probe.style.cssText = `position:absolute;top:0;left:0;width:1px;height:${PROBE_HEIGHT}px;visibility:hidden;pointer-events:none`;
    viewport.appendChild(probe);
    const cap = capFrom(probe.getBoundingClientRect().height);
    probe.remove();
    capTrusted = cap !== null;
    domMax = cap ?? CAP_FALLBACK;
  }
```

- [ ] **Step 2: `scrollToOffset` through the map**

Replace the body from `const row = rows[i];` to the end of the function with:

```ts
    const row = rows[i];
    // Where the grid is now, read from the viewport rather than from `scrollTop`, which
    // trails it by a scroll event.
    const now = map.virtualAt(viewport.scrollTop);
    if (align === 'start') {
      const header = rows[i - 1];
      scrollToVirtual(header?.kind === 'header' && header.first === row.first ? header.top : row.top);
    } else if (row.top < now) {
      scrollToVirtual(row.top);
    } else if (row.top + row.height > now + height) {
      scrollToVirtual(row.top + row.height - height);
    }
    // (keep the existing comment block about re-pinning here, then:)
    pinned = firstVisibleOffset(rows, map.virtualAt(viewport.scrollTop));
```

The existing comment's last sentence ("Read back from `viewport`, not from the value written above: the browser clamps…") stays true: `virtualAt(viewport.scrollTop)` is the read-back, through the map.

- [ ] **Step 3: The effects - order matters**

Directly *above* the pin effect (`let pinnedWidth = gridSize.width;`), so a tile-size change resizes the map before the pin restore writes through it:

```ts
  // After the canvas has its new height (`$effect`, not `$effect.pre`): a write into a canvas
  // not yet grown is clamped away. Above the pin effect, which scrolls through the map.
  $effect(() => {
    const w = map.resize(total, height, domMax);
    if (w !== null) writeDom(w);
  });

  $effect(() => {
    measureCap();
    // A new display scale moves Chromium's cap in CSS px (it applies it in device pixels).
    let query: MediaQueryList | null = null;
    const listen = () => {
      query?.removeEventListener('change', onScale);
      query = matchMedia(`(resolution: ${devicePixelRatio}dppx)`);
      query.addEventListener('change', onScale);
    };
    const onScale = () => {
      measureCap();
      listen();
    };
    listen();
    return () => query?.removeEventListener('change', onScale);
  });

  // A reading taken before the window was laid out is taken again on the next resize.
  $effect(() => {
    void width;
    void height;
    if (!capTrusted) measureCap();
  });

  // Scrolling has gone still: put the thumb back where the grid is.
  $effect(() => {
    if (speed.motion.kind !== 'still') return;
    const w = map.settle();
    if (w !== null) writeDom(w);
  });
```

`measureCap` reads no `$state`, and `writeDom` writes `shift`/`scrollTop`, which none of these effects read, so none re-runs itself.

- [ ] **Step 4: The scroll event, the press, `atCanvas`, the timeline**

Replace the viewport's `onscroll` with:

```svelte
    onscroll={(e) => {
      const w = map.onScroll(viewport.scrollTop);
      if (w !== null) writeDom(w);
      scrollTop = map.virtual;
      shift = map.shift;
      speed.sample(scrollTop, e.timeStamp, height);
    }}
```

Replace `onpointerdown={bandDown}` with:

```svelte
    onpointerdown={(e) => {
      // Past the cap, a drag on the scrollbar maps proportionally (`scroll-map.ts`).
      map.press(e.offsetX > viewport.clientWidth);
      bandDown(e);
    }}
```

In `<svelte:window …>`, replace `onpointerup={bandUp}` with:

```svelte
  onpointerup={(e) => {
    map.release();
    bandUp(e);
  }}
  onpointercancel={() => map.release()}
  onpointermove={(e) => {
    // A native scrollbar can keep its release to itself: a pointer moving with no button
    // held says the press is over, so the next wheel step is not read as a thumb drag.
    if (e.buttons === 0) map.release();
  }}
```

In `atCanvas`: `y: y - box.top + map.virtualAt(viewport.scrollTop)`.

Band autoscroll (`if (whole !== 0) viewport.scrollTop += whole;`) stays as it is: its step is at most `BAND_SCROLL_MAX × BAND_FRAME_MAX_MS / 1000` = 70 px, a relative step through `onscroll`, and `atCanvas` right after it reads through `virtualAt`. Add a one-line comment saying so.

Timeline: `onscrub={(top) => scrollToVirtual(top)}`.

- [ ] **Step 5: Drawing at `row.top − shift`**

- `.canvas`: `style:height="{domHeight}px"`.
- header: `style:top="{row.top - shift}px"`.
- row: `style:top="{row.top - shift}px"`.
- band: `style:top="{Math.min(band.y0, band.y1) - shift}px"`.

Keep `{#each rendered as row (row.top)}` keyed on the virtual `row.top`.

- [ ] **Step 6: Verify**

Run: `npm run check && npm test`
Expected: 0 errors, 0 warnings; all tests pass.

Run: `cargo run -p xtask -- scroll-probe`
Expected: all four cases `ok`.

Revert probes (Grid wiring, each an exact replacement, then restore):
- `style:top="{row.top - shift}px"` (row) → `style:top="{row.top}px"` - expect a huge case to fail (rows land past the canvas).
- the `map.resize` effect's `if (w !== null) writeDom(w);` → `` - expect a huge case to fail (entering the mapped range never anchors).
- `onscrub={(top) => scrollToVirtual(top)}` → `onscrub={(top) => (viewport.scrollTop = top)}` - the probe does not cover the scrub; this is on the smoke checklist. Record in the commit message that the scrub has no automated check.

- [ ] **Step 7: Commit**

```bash
git add ui/src/components/Grid.svelte
git commit -m "fix(grid): every photo reachable in a library taller than the layout cap

The grid's scrollTop is now the layout's position, mapped onto a canvas held
under the engine's measured cap (scroll-map.ts). Under the cap the map is the
identity. scroll-probe reaches offset 299,999 of 300,000 at one column, and at
display scale 2. The timeline scrub, the thumb settling and the feel of each
webview's kinetic scrolling have no automated check: README smoke checklist.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: Docs - smoke checklist, CLAUDE.md, the audit plan

**Files:**
- Modify: `README.md` (`## Manual smoke checklist`)
- Modify: `CLAUDE.md`
- Modify: `docs/superpowers/plans/2026-09-28-performance-audit-open-items.md`

- [ ] **Step 1: README smoke checklist**

Add, in the checklist's grid section (match its bullet style):

```markdown
- **A library taller than the browser allows** (`cargo run -p xtask -- scroll-probe` checks
  the end is reachable; this checks how it feels). On a library of 150,000+ photos, or at
  large tiles in the narrowest window, and on Windows at 100%, 150% and 200% scaling:
  wheel and trackpad scroll at their usual speed; dragging the scrollbar thumb moves through
  the whole library and reaches both ends; after a pause the thumb jumps to where the grid
  is without the photos moving; End, Home, a folder jump and a timeline scrub land where
  they should; a rubber band dragged past the edge keeps selecting what it drew. On a
  normal library, nothing about scrolling has changed.
```

- [ ] **Step 2: CLAUDE.md**

In `## Architecture`, after the paragraph "**A grid offset is only meaningful against one index version.**", add:

```markdown
**The grid's `scrollTop` is the layout's position, not the viewport's.** Engines cap a box
at 33,554,428 px (less in CSS px on a scaled Windows display), and a large library at large
tiles in a narrow window is taller; past the cap `.canvas` is held at a measured
`domHeight` and `scroll-map.ts` maps the layout's position onto it, drawing rows at
`row.top - shift`. Under the cap the map is the identity. Anything new that reads or writes
`viewport.scrollTop` in `Grid.svelte` goes through the map (`virtualAt`, `scrollToVirtual`,
`writeDom`) - a direct read is a DOM position, and in a big library it names another photo.
`cargo run -p xtask -- scroll-probe` checks the end of a 300,000-photo library is reachable.
```

- [ ] **Step 3: The audit plan**

In `docs/superpowers/plans/2026-09-28-performance-audit-open-items.md`, change the section 6 heading to `## 6. The grid canvas exceeds the browser's layout-height limit — fixed` and add one line under it: `Fixed per docs/superpowers/specs/2026-09-28-photon-grid-canvas-cap-design.md (scroll-map.ts, xtask scroll-probe). The measurements below are the before.`

- [ ] **Step 4: Commit**

```bash
git add README.md CLAUDE.md docs/superpowers/plans/2026-09-28-performance-audit-open-items.md
git commit -m "docs: the grid's virtual scroll position, and its smoke checks

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

## After the tasks

A whole-branch review before the PR (CLAUDE.md, "A large branch gets an independent read"): point the reviewer at `Grid.svelte`'s effect order, at every remaining direct `viewport.scrollTop` use (there should be exactly: the map's writes in `writeDom`, the reads passed to `map.onScroll`/`virtualAt`, and band autoscroll's relative step), and at what the map *arms* in old code - the pin restore and the scroll-clamp trap.
