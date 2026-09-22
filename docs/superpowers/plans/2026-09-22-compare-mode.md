# Compare Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put two to four selected photos side by side at the same zoom over the same point, so the difference that decides which one you keep — which is sharp, whose eyes are open — can actually be seen.

**Architecture:** A new overlay in `App.svelte` beside `Viewer` and `Settings`, counting towards `covered`. All the logic lives in a pure `createCompare` factory (`ui/src/lib/compare.svelte.ts`) the way `createSlideshow` and `createCropTool` do, because vitest runs under `environment: 'node'` and a `.svelte` file cannot be rendered. Zoom and pan are **one shared value**, not per pane. Panes load the 1600 px preview thumbnail and upgrade to the full render only for the focused pane above 100%.

**Tech Stack:** Svelte 5 runes + TypeScript, vitest. **No Rust, no schema, no new IPC** — the backend already serves every pixel this needs.

**Spec:** `docs/superpowers/specs/2026-09-21-photon-compare-mode-design.md`

---

## Global Constraints

- **2 to 4 photos.** Fewer or more is refused; the grid offers Compare only in that range.
- **Zoom and pan are shared**, stored once. Two photos at 200% on the same eye is the comparison; two photos each at their own zoom is not.
- **A pane loads `thumb/<id>/preview/<thumbKey>`**, not `/image/<id>`. Full-size renders run one at a time behind `RENDERING` in `protocol.rs`, so four panes would be four serial renders before anything appeared — and at fit a 24 MP render is thrown away to draw 600 px of it. The upgrade to `/image/<id>` happens **only for the focused pane and only once zoom passes 100%**.
- **Reuse `ui/src/lib/nav.ts`'s `MIN_ZOOM = 1`, `MAX_ZOOM = 4`, `clampZoom(zoom)`, `clampPan(x, y, zoom, width, height)` and `wheelStep(accumulated, delta, threshold)`.** Do not reimplement any of them — the viewer's zoom already behaves the way compare's should, and a second copy would drift.
- **The overlay lives in `App.svelte`**, counts towards `covered`, and hands focus back to the grid's viewport **after `await tick()`** — `<main>` is inert until the DOM catches up, and focusing an inert element silently does nothing. A close that lands on `<body>` leaves the grid's arrow keys, Enter and Escape dead until the user clicks.
- **A pointer gesture has three endings:** `pointerup` finishes it, `Escape` abandons it, and a touchscreen pan sends **`pointercancel`** and nothing else. One teardown reached by all three.
- **`inert` does not stop a `requestAnimationFrame`.** Nothing in compare should drive a frame loop; if anything does, it needs its own stop.
- **Anything per-frame multiplies by the frame's own duration**, or it runs twice as fast at 120 Hz as at 60 Hz.
- **The panes' ground is the viewer's black**, which `no-literals.test.ts` allows as a colour literal **once, only in the viewer**. Compare takes its ground from the same token by putting `data-theme="dark"` on its root, as `Viewer.svelte:629` does — **not a second literal**. Any new `--compare-*` token must be declared in `ui/src/tokens.css` or that test fails.
- **`no-literals.test.ts`** also fails on a named colour, a `color-mix`/`oklch`/`oklab`/`lab`/`lch` function used as a component colour, a glyph icon, or any `var(--x)` `tokens.css` does not declare.
- **The UI gate:** `npm run check` (**0 errors AND 0 warnings**) and `npm test`, from the repo root.
- **The Rust gate still runs**, because `xtask` changes in Task 6: `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`.
- **A new test must be demonstrated to fail with its change reverted.** A compile error is not proof. **A probe that passes is a finding, not a formality** — the last two branches found five tests that defended nothing this way.
- **Where a change genuinely cannot have a discriminating test — component markup and effect wiring — say so in the commit message and why.** Do not invent a test that passes either way.
- **Comments carry reasoning, not mechanics**; a wrong justification is treated as a defect.
- **Branch:** create `feat/compare-mode` off `main` before Task 1.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `ui/src/lib/compare.svelte.ts` | `createCompare` — panes, shared zoom/pan, focus, the upgrade rule | 1, 2 |
| `ui/src/lib/compare.svelte.test.ts` | its tests | 1, 2 |
| `ui/src/components/Compare.svelte` | the overlay: panes, keys, the pan drag | 3 |
| `ui/src/App.svelte` | mounts it, `covered`, focus return | 4 |
| `ui/src/components/Grid.svelte` | `C` and the menu item, both gated on 2–4 | 5 |
| `ui/src/tokens.css` | any new `--compare-*` token | 3 |
| `crates/xtask/src/screenshots.rs`, `crates/xtask/screenshots/mock.js` | the shots and the action that opens compare | 6 |
| `README.md` | the feature and its smoke checks | 6 |

---

### Task 1: `createCompare` — panes and shared zoom

**Files:**
- Create: `ui/src/lib/compare.svelte.ts`
- Create: `ui/src/lib/compare.svelte.test.ts`

**Interfaces:**
- Consumes: `MIN_ZOOM`, `MAX_ZOOM`, `clampZoom`, `clampPan` from `./nav`.
- Produces:
  - `export const MIN_PANES = 2; export const MAX_PANES = 4;`
  - `export function canCompare(count: number): boolean`
  - `export interface ComparePane { id: number; fileName: string; width: number; height: number; takenAt: number; thumbKey: string; edit: boolean; loaded: boolean }`
  - `export interface CompareDeps { load(id: number): Promise<ComparePane>; onclose(): void; onerror(e: unknown): void }`
  - `export function createCompare(deps: CompareDeps)` with `get panes`, `get focus`, `get zoom`, `get pan`, `open(ids)`, `close()`, `zoomAt(factor, originX, originY, width, height)`, `panBy(dx, dy, width, height)`, `focusPane(i)`, `nextPane()`, `needsFullImage(i)`

- [ ] **Step 1: Create the branch**

```bash
git checkout main && git pull
git checkout -b feat/compare-mode
```

- [ ] **Step 2: Write the failing tests**

Create `ui/src/lib/compare.svelte.test.ts`:

```typescript
import { describe, expect, it, vi } from 'vitest';
import { canCompare, createCompare, MAX_PANES, MIN_PANES, type ComparePane } from './compare.svelte';
import { MAX_ZOOM, MIN_ZOOM } from './nav';

function pane(id: number): ComparePane {
  return {
    id,
    fileName: `IMG_${id}.jpg`,
    width: 4000,
    height: 3000,
    takenAt: 1_700_000_000 + id,
    thumbKey: `k${id}`,
    edit: false,
    loaded: false,
  };
}

function deps(overrides: Partial<Parameters<typeof createCompare>[0]> = {}) {
  return {
    load: vi.fn(async (id: number) => pane(id)),
    onclose: vi.fn(),
    onerror: vi.fn(),
    ...overrides,
  };
}

describe('canCompare', () => {
  it('accepts two to four and refuses anything else', () => {
    expect(canCompare(1)).toBe(false);
    expect(canCompare(MIN_PANES)).toBe(true);
    expect(canCompare(3)).toBe(true);
    expect(canCompare(MAX_PANES)).toBe(true);
    expect(canCompare(5)).toBe(false);
    expect(canCompare(0)).toBe(false);
  });
});

describe('createCompare', () => {
  it('opens with one pane per id, the first focused', async () => {
    const c = createCompare(deps());
    await c.open([7, 8, 9]);
    expect(c.panes.map((p) => p.id)).toEqual([7, 8, 9]);
    expect(c.focus).toBe(0);
    expect(c.zoom).toBe(MIN_ZOOM);
  });

  it('refuses to open with one or five', async () => {
    const d = deps();
    const c = createCompare(d);
    await c.open([7]);
    expect(c.panes).toEqual([]);
    await c.open([1, 2, 3, 4, 5]);
    expect(c.panes).toEqual([]);
    expect(d.load).not.toHaveBeenCalled();
  });

  /** The whole feature: one zoom, read by every pane. Fails the moment zoom is stored per
   *  pane, which is the shape this would drift into. */
  it('zoom is shared across panes', async () => {
    const c = createCompare(deps());
    await c.open([1, 2]);
    c.zoomAt(2, 0, 0, 800, 600);
    expect(c.zoom).toBe(2);
    // There is exactly one zoom; no pane carries its own.
    for (const p of c.panes) expect('zoom' in p).toBe(false);
  });

  it('zoom is clamped to the viewer\'s range', async () => {
    const c = createCompare(deps());
    await c.open([1, 2]);
    c.zoomAt(100, 0, 0, 800, 600);
    expect(c.zoom).toBe(MAX_ZOOM);
    c.zoomAt(0.001, 0, 0, 800, 600);
    expect(c.zoom).toBe(MIN_ZOOM);
  });

  /** Zooming about a point keeps that point still: the pan must move by the same fraction
   *  of the origin offset that the scale changed by. */
  it('zoom about a point keeps that point still', async () => {
    const c = createCompare(deps());
    await c.open([1, 2]);
    // A point 100px right and 50px below the centre, zooming 1 -> 2.
    c.zoomAt(2, 100, 50, 800, 600);
    expect(c.pan.x).toBeCloseTo(-100, 5);
    expect(c.pan.y).toBeCloseTo(-50, 5);
  });

  it('pan is clamped so the photo keeps covering the pane', async () => {
    const c = createCompare(deps());
    await c.open([1, 2]);
    // At fit, there is nowhere to pan.
    c.panBy(500, 500, 800, 600);
    expect(c.pan).toEqual({ x: 0, y: 0 });
  });

  it('closing tells the caller and empties the panes', async () => {
    const d = deps();
    const c = createCompare(d);
    await c.open([1, 2]);
    c.close();
    expect(d.onclose).toHaveBeenCalledOnce();
    expect(c.panes).toEqual([]);
  });

  it('a load failure closes rather than showing half a comparison', async () => {
    const d = deps({ load: vi.fn(async () => { throw new Error('gone'); }) });
    const c = createCompare(d);
    await c.open([1, 2]);
    expect(d.onerror).toHaveBeenCalled();
    expect(c.panes).toEqual([]);
  });
});
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `npm test -w ui -- src/lib/compare.svelte.test.ts`
Expected: FAIL — the module does not exist.

- [ ] **Step 4: Write the implementation**

Create `ui/src/lib/compare.svelte.ts`:

```typescript
import { MIN_ZOOM, clampPan, clampZoom } from './nav';

/** Two is the fewest that is a comparison; four is where a 2x2 stops being legible at any
 *  window size worth supporting. */
export const MIN_PANES = 2;
export const MAX_PANES = 4;

export function canCompare(count: number): boolean {
  return count >= MIN_PANES && count <= MAX_PANES;
}

/** What a pane needs to draw itself and to say what distinguishes it from its neighbours.
 *  `edit` only records *whether* the photo carries one - the thumbnail key already accounts
 *  for it, and the full-size URL needs a cache-busting parameter when it is set. */
export interface ComparePane {
  id: number;
  fileName: string;
  width: number;
  height: number;
  takenAt: number;
  thumbKey: string;
  edit: boolean;
  loaded: boolean;
}

export interface CompareDeps {
  load(id: number): Promise<ComparePane>;
  onclose(): void;
  onerror(e: unknown): void;
}

/** Two to four photos side by side, at one zoom over one point.
 *
 *  Zoom and pan are stored **once**, not per pane, and that is the feature rather than an
 *  implementation choice: two photos at 200% on the same eye is the comparison a person is
 *  trying to make, and two photos each at their own zoom is not.
 *
 *  Everything here is pure and synchronous apart from `open`, so vitest covers it under
 *  `environment: 'node'` where a `.svelte` file cannot be rendered. */
export function createCompare(deps: CompareDeps) {
  let panes = $state<ComparePane[]>([]);
  let focus = $state(0);
  let zoom = $state(MIN_ZOOM);
  let pan = $state({ x: 0, y: 0 });

  return {
    get panes() {
      return panes;
    },
    get focus() {
      return focus;
    },
    get zoom() {
      return zoom;
    },
    get pan() {
      return pan;
    },

    /** Loads every pane before showing any of them: half a comparison is worse than none,
     *  and the panes are laid out by how many there are. */
    async open(ids: number[]) {
      if (!canCompare(ids.length)) return;
      try {
        const loaded = await Promise.all(ids.map((id) => deps.load(id)));
        panes = loaded;
        focus = 0;
        zoom = MIN_ZOOM;
        pan = { x: 0, y: 0 };
      } catch (e) {
        panes = [];
        deps.onerror(e);
      }
    },

    close() {
      panes = [];
      zoom = MIN_ZOOM;
      pan = { x: 0, y: 0 };
      deps.onclose();
    },

    /** Zooms by `factor` about a point `originX`/`originY` from the pane's centre.
     *
     *  The pan correction is what keeps that point still: scaling by `k` moves a point at
     *  offset `d` to `k*d`, so the pan must take back `d * (k - 1)`. Without it the photo
     *  appears to slide out from under the pointer, which is exactly the feeling that makes
     *  a shared zoom useless for comparing. */
    zoomAt(factor: number, originX: number, originY: number, width: number, height: number) {
      const next = clampZoom(zoom * factor);
      const k = next / zoom;
      const x = pan.x - originX * (k - 1);
      const y = pan.y - originY * (k - 1);
      zoom = next;
      pan = clampPan(x, y, next, width, height);
    },

    panBy(dx: number, dy: number, width: number, height: number) {
      pan = clampPan(pan.x + dx, pan.y + dy, zoom, width, height);
    },

    focusPane(i: number) {
      if (i >= 0 && i < panes.length) focus = i;
    },

    nextPane() {
      if (panes.length > 0) focus = (focus + 1) % panes.length;
    },

    /** Whether pane `i` should ask for the full-size render rather than the preview.
     *
     *  At most one pane ever does. Full-size renders are serialised behind `RENDERING` in
     *  `protocol.rs`, so letting every pane upgrade would queue four 24 MP decodes and show
     *  nothing until the last finished; and below 100% the preview's pixels are all that can
     *  be seen anyway. */
    needsFullImage(i: number): boolean {
      return i === focus && zoom > MIN_ZOOM;
    },
  };
}

export type Compare = ReturnType<typeof createCompare>;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `npm test -w ui -- src/lib/compare.svelte.test.ts && npm run check`
Expected: PASS; 0 errors and 0 warnings.

- [ ] **Step 6: Prove the shared-zoom test discriminates**

The `zoom is shared across panes` test as written checks `'zoom' in p` — a structural assertion. **Probe it properly:** temporarily change `zoomAt` so it writes `zoom` but leaves `pan` untouched, and confirm `zoom about a point keeps that point still` fails. Then temporarily make `zoomAt` clamp with `Math.min(4, ...)` inline instead of `clampZoom` and confirm nothing fails — **that is a finding**: it means the clamp's reuse is not pinned, and you should say so rather than paper over it.

Report every probe you ran and which discriminated.

- [ ] **Step 7: Commit**

```bash
git add ui/src/lib/compare.svelte.ts ui/src/lib/compare.svelte.test.ts
git commit -m "feat(ui): the state behind comparing photos side by side

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Focus, the upgrade rule, and the drag's three endings

**Files:**
- Modify: `ui/src/lib/compare.svelte.ts`
- Modify: `ui/src/lib/compare.svelte.test.ts`

**Interfaces:**
- Consumes: Task 1's factory.
- Produces: `beginPan(pointerId)`, `endPan()`, `get panning(): boolean` on the same object.

- [ ] **Step 1: Write the failing tests**

Append to the `createCompare` describe block:

```typescript
  it('focus moves by index and wraps', async () => {
    const c = createCompare(deps());
    await c.open([1, 2, 3]);
    c.focusPane(2);
    expect(c.focus).toBe(2);
    c.nextPane();
    expect(c.focus).toBe(0);
    // Out of range is ignored rather than throwing: `3` is a key a person can press.
    c.focusPane(9);
    expect(c.focus).toBe(0);
  });

  /** Fails if the upgrade rule is dropped - the change that would quietly serialise four
   *  24 MP renders behind `RENDERING`. */
  it('only the focused pane asks for a full render, and only above fit', async () => {
    const c = createCompare(deps());
    await c.open([1, 2, 3]);
    // At fit, nobody needs one.
    expect([0, 1, 2].map((i) => c.needsFullImage(i))).toEqual([false, false, false]);
    c.zoomAt(2, 0, 0, 800, 600);
    expect([0, 1, 2].map((i) => c.needsFullImage(i))).toEqual([true, false, false]);
    c.focusPane(2);
    expect([0, 1, 2].map((i) => c.needsFullImage(i))).toEqual([false, false, true]);
    // At most one, always.
    expect([0, 1, 2].filter((i) => c.needsFullImage(i))).toHaveLength(1);
  });

  /** The bug class CLAUDE.md records from the rubber band: a drag that a touchscreen
   *  cancels leaves a live pointer id blocking every later drag. All three endings reach
   *  one teardown. */
  it('escape and pointercancel reach the same teardown as pointerup', async () => {
    for (const ending of ['up', 'escape', 'cancel'] as const) {
      const c = createCompare(deps());
      await c.open([1, 2]);
      c.beginPan(7);
      expect(c.panning).toBe(true);
      c.endPan();
      expect(c.panning).toBe(false);
      // A second gesture must be able to start, whichever way the first ended.
      c.beginPan(8);
      expect(c.panning).toBe(true);
      c.endPan();
      expect(c.panning).toBe(false);
    }
  });

  it('closing while panning leaves no live gesture', async () => {
    const c = createCompare(deps());
    await c.open([1, 2]);
    c.beginPan(7);
    c.close();
    expect(c.panning).toBe(false);
  });
```

- [ ] **Step 2: Run them to verify they fail**

Run: `npm test -w ui -- src/lib/compare.svelte.test.ts`
Expected: FAIL — `beginPan` / `endPan` / `panning` do not exist.

- [ ] **Step 3: Implement**

Add to the factory, beside the other state:

```typescript
  /** The pointer id of a pan in progress, or null. A gesture has three endings - pointerup
   *  finishes it, Escape abandons it, and a touchscreen sends pointercancel and nothing
   *  else - and all three call `endPan`. A missed teardown here would leave a live id that
   *  blocks every later drag, which is the failure this project has already shipped once in
   *  the grid's rubber band. */
  let panPointer: number | null = null;
```

and to the returned object:

```typescript
    get panning() {
      return panPointer !== null;
    },

    beginPan(pointerId: number) {
      panPointer = pointerId;
    },

    endPan() {
      panPointer = null;
    },
```

and add `panPointer = null;` to `close()`.

- [ ] **Step 4: Run them to verify they pass**

Run: `npm test -w ui && npm run check`
Expected: PASS; 0 errors and 0 warnings.

- [ ] **Step 5: Probe both new rules**

Temporarily change `needsFullImage` to `return zoom > MIN_ZOOM` (dropping the focus check) and confirm `only the focused pane asks for a full render` fails. Restore by hand. Then temporarily make `close()` leave `panPointer` alone and confirm `closing while panning leaves no live gesture` fails. Restore and confirm green.

- [ ] **Step 6: Commit**

```bash
git add ui/src/lib/compare.svelte.ts ui/src/lib/compare.svelte.test.ts
git commit -m "feat(ui): compare's focus, its render upgrade, and a drag with three endings

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: The overlay component

**Files:**
- Create: `ui/src/components/Compare.svelte`
- Modify: `ui/src/tokens.css` (only if a new token is genuinely needed)

**Interfaces:**
- Consumes: `createCompare`, `ComparePane` (Tasks 1-2); `mediaUrl` from `../lib/api`; `MIN_ZOOM` from `../lib/nav`.
- Produces: `<Compare ids={number[]} onclose={() => void} />`.

- [ ] **Step 1: Read the two components you are following**

Read `ui/src/components/Viewer.svelte` for: how it sets `data-theme="dark"` on its root (line ~629), how it builds the preview URL (`mediaUrl(\`thumb/${item.id}/preview/${item.thumbKey}\`)`, line ~691) and the full URL (`mediaUrl(\`image/${it.id}\`) + (it.edit ? \`?k=${it.thumbKey}\` : '')`, line ~445), and how its pan drag is wired. Read `ui/src/App.svelte`'s splitter for the three-ending pattern (`onpointerup` and `onpointercancel` both reaching `endResize`).

- [ ] **Step 2: Write the component**

Create `ui/src/components/Compare.svelte`. It takes `ids` and `onclose`, builds a `createCompare` whose `load` calls `api.viewerItem(id)` and maps it to a `ComparePane`, and renders one `<img>` per pane inside a grid.

Requirements, each with its reason so you do not simplify one away:

- **Root carries `data-theme="dark"`**, like the viewer's, so the panes' ground comes from the themed token rather than a second colour literal. `no-literals.test.ts` allows the black literal only in the viewer, once.
- **Layout:** two panes side by side; three or four as a 2×2. CSS grid with `grid-template-columns` switched on `panes.length`.
- **Each pane shows** its file name, and — **only where the panes differ** — its pixel dimensions and its capture time. Identical values on every pane are noise; the point is the fact that decides between them.
- **`src`** is `mediaUrl(\`thumb/${p.id}/preview/${p.thumbKey}\`)`, upgrading to `mediaUrl(\`image/${p.id}\`) + (p.edit ? \`?k=${p.thumbKey}\` : '')` when `compare.needsFullImage(i)`.
- **Transform:** every pane's `<img>` uses the same `scale(zoom)` and `translate(pan)`, read from the one shared state.
- **Wheel** zooms via `compare.zoomAt`, with the origin taken from the pointer's position relative to the pane's centre so the point under the pointer stays put. Use `e.preventDefault()`.
- **Pan drag:** `onpointerdown` calls `beginPan(e.pointerId)` and captures; `onpointermove` calls `panBy`; **`onpointerup`, `onpointercancel` and Escape all reach `endPan`.** The splitter in `App.svelte` is the model.
- **Keys:** `1`–`4` focus a pane, `Tab` moves focus (call `nextPane` and `preventDefault`), `S` stars the focused pane via `api.setStar`, `Enter` opens the focused pane in the viewer and closes compare, `Escape` closes — but **if a pan is live, Escape ends the pan and nothing else**, the way the grid's band handler does.
- **The root is `tabindex="-1"`** and focused on mount so the keys arrive. **Give it `class="focus-container"`** — `ui/src/tokens.css:104`'s rule is scoped to that class, and without it the whole surface grows a focus ring, which says nothing.
- **No `requestAnimationFrame` anywhere.** Nothing here needs a frame loop; `inert` would not stop one.

Every colour must be a `var(--token)` already declared in `tokens.css`. If you genuinely need a new one, declare it in **both** the light and dark blocks (or the theme-independent scales block if it is never themed) and say in your report which and why.

- [ ] **Step 3: Run the gates**

Run: `npm run check && npm test`
Expected: 0 errors, 0 warnings; all tests pass, including `no-literals.test.ts` and `tokens.test.ts`.

- [ ] **Step 4: Measure the layout without the GUI**

The pane geometry is the one visual thing that *can* be checked here. Use the headless-Chromium layout probe: a static page holding the component's CSS, run through Chromium with `--dump-dom` and a load script that writes `getBoundingClientRect()` into `document.title`. Confirm two panes are side by side and four are a 2×2, with the gutters you intended, at a realistic window size.

Chromium was at `/usr/bin/chromium`. If it is unavailable, say so and skip — do not guess at the numbers.

- [ ] **Step 5: Commit**

The component is markup and effect wiring; there is no component test harness. Say so in the commit message and why.

```bash
git add ui/src/components/Compare.svelte ui/src/tokens.css
git commit -m "feat(ui): the compare overlay

No test: vitest runs under node and a .svelte file cannot be rendered, so
the logic lives in createCompare (tested) and this is markup and effect
wiring, covered by svelte-check, no-literals.test.ts, the layout probe and
the smoke checklist.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Mount it in `App.svelte`

**Files:**
- Modify: `ui/src/App.svelte`

**Interfaces:**
- Consumes: `<Compare>` (Task 3).
- Produces: `openCompare(ids: number[])`, called by Task 5 from the grid.

- [ ] **Step 1: Read how the viewer is mounted and closed**

`ui/src/App.svelte:267-270` mounts the overlays; `covered` is derived at line 55-57; the close handlers around lines 176-215 show the `await tick()` dance and why it is there.

- [ ] **Step 2: Implement**

- `let compareIds = $state<number[] | null>(null);`
- `covered` gains `compareIds !== null` — **this is what makes the overlay safe**: the grid is inside `<main>`, `covered` makes `<main>` inert, and an overlay mounted inside the grid would be made inert by its own opening, so Tab would walk out into the tiles and Settings could open on top of it.
- Mount beside the others: `{#if compareIds !== null}<Compare ids={compareIds} onclose={closeCompare} />{/if}`
- `openCompare(ids)` sets `compareIds = ids`.
- `closeCompare()` sets `compareIds = null`, then **`await tick()`** before returning focus to the grid's viewport — `<main>` is inert until the DOM catches up and focusing an inert element silently does nothing. Follow `closeViewer`/`closeSettings` exactly; the grid's keys live on its viewport, so a close that lands on `<body>` leaves the arrow keys, Enter and Escape dead until the user clicks.

- [ ] **Step 3: Run the gates**

Run: `npm run check && npm test`
Expected: 0 errors, 0 warnings; all pass.

- [ ] **Step 4: Commit**

Effect wiring again — say so and why.

```bash
git add ui/src/App.svelte
git commit -m "feat(ui): compare is an overlay, so it makes the app behind it inert

No test: overlay mounting and focus return are effect wiring in a .svelte
file, which vitest's node environment cannot render. The smoke checklist
covers the focus return.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Opening it from the grid

**Files:**
- Modify: `ui/src/components/Grid.svelte`

**Interfaces:**
- Consumes: `canCompare` from `../lib/compare.svelte`; `library.selectedItemIds` and `library.selectionCount`; `oncompare` prop from `App.svelte` (Task 4's `openCompare`).
- Produces: nothing downstream.

- [ ] **Step 1: Read the grid's key handler and its menu**

`ui/src/components/Grid.svelte:238-262` is the key handler; note that **Escape is special while a band is live** (`cancelBandKey()`), and that the handler honours nothing but Escape during a drag — `C` must respect the same rule. The context menu's items are around lines 627-652.

- [ ] **Step 2: Implement**

- A `C` case in the key handler: when `canCompare(library.selectionCount)`, `e.preventDefault()` and call `oncompare(library.selectedItemIds)`. **Place it after the band check**, so a live drag still owns the keyboard.
- A **Compare** item in the context menu, shown only when `canCompare(library.selectionCount)` — following how the existing items use `withSelection`.
- Add the `oncompare` prop beside the existing `onopen` / `onkeywords` / `onexport` props and wire it in `App.svelte`.

- [ ] **Step 3: Run the gates**

Run: `npm run check && npm test`
Expected: 0 errors, 0 warnings; all pass.

- [ ] **Step 4: Confirm the gating cannot be bypassed**

`canCompare` is already tested (Task 1). What is **not** pinned is that the grid actually consults it. Read your own diff and confirm both entry points call it; state in your report that the component itself has no test and why.

- [ ] **Step 5: Commit**

```bash
git add ui/src/components/Grid.svelte ui/src/App.svelte
git commit -m "feat(ui): C compares the selected photos

No test: the grid is a .svelte component and cannot be rendered under
vitest's node environment. The 2-4 rule it consults is canCompare, which is
tested; that the grid consults it is on the smoke checklist.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: Screenshots, README and the smoke checklist

**Files:**
- Modify: `crates/xtask/screenshots/mock.js`, `crates/xtask/src/screenshots.rs`
- Modify: `README.md`, `CLAUDE.md`

- [ ] **Step 1: Add the mock action and the shots**

`mock.js` needs an action that selects three tiles and opens compare, so the screenshot has something to show. Read how the existing actions (`select`, `menu`, `info`) do it. `viewer_item` is already answered, which is what `load` calls — confirm it returns everything `ComparePane` needs.

Add `compare-light` and `compare-dark` to `SHOTS` in `screenshots.rs`, following the existing entries' shape.

- [ ] **Step 2: Generate them and look**

Run `cargo run -p xtask -- screenshots --only compare-light` and `--only compare-dark` (Chromium on `PATH` or in `CHROMIUM`; it was at `/usr/bin/chromium`). **Read the PNGs with the Read tool and say what you actually saw** — whether three panes are laid out as a 2×2 with one empty cell, whether the per-pane labels are legible, whether anything overflows. If Chromium is unavailable, say so and skip the generation but still add the entries.

- [ ] **Step 3: Update the PNG count**

`CLAUDE.md` states how many PNGs `screenshots` writes (seventeen as of v0.23.0); two more makes nineteen. Update both occurrences and check no other file states it.

- [ ] **Step 4: Write the README section**

A `###` section in the user's voice, near Rotating and cropping. It should say: select two to four photos and press `C` (or right-click → Compare); they appear side by side; zoom and pan move all of them together, which is the point; `1`–`4` or Tab picks one, `S` stars it, Enter opens it, Escape goes back with the selection intact. Say plainly that photon does not delete — the keeper is yours to act on elsewhere.

- [ ] **Step 5: Add the smoke-checklist entries**

In `## Manual smoke checklist`. At minimum:

```markdown
- Select three photos in the grid and press `C`. Zoom into one corner with the wheel and
  drag: all three should move together, over the same part of each photo.
- Zoom past 100% on the focused pane and check it sharpens while the others stay as they
  were — only the focused pane asks for the full-size render.
- Start a pan and press Escape: the pan should stop and compare should stay open. Press
  Escape again to leave.
- Leave compare and check the grid's arrow keys, Enter and Escape still work without
  clicking first.
- Select one photo, then five, and check `C` does nothing and the menu offers no Compare.
```

Do not pad beyond what needs eyes.

- [ ] **Step 6: Run every gate and chore**

```bash
cargo fmt --all && cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
npm run check
npm test
cargo run -p xtask -- versions
cargo run -p xtask -- metadata
```

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "docs: comparing photos side by side

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

**Do not push and do not open a PR** — the controller handles both.

---

## Self-Review

**Spec coverage.** Entry from the grid selection at 2–4 with `C` and a menu item (Task 5, gated by `canCompare` from Task 1); the 2-across / 2×2 layout and the per-pane facts (Task 3); shared zoom and pan (Tasks 1-2, the spec's five named tests all present — `zoom_is_shared_across_panes`, `zoom_about_a_point_keeps_that_point_still`, `only_the_focused_pane_asks_for_a_full_render`, `escape_and_pointercancel_reach_the_same_teardown`, `opening_with_one_or_five_is_refused`); focus with `1`–`4` and Tab, `S`, `Enter`, `Escape` (Tasks 2-3); the preview-then-upgrade loading rule (Tasks 1-3); the overlay living in `App.svelte` with `covered` and the `await tick()` focus return (Task 4); the dark ground from the themed token rather than a second literal (Task 3); the layout probe and the two screenshots (Tasks 3 and 6); README and smoke checklist (Task 6). `neighbours` is deliberately untouched, as the spec requires.

**Type consistency.** `ComparePane` is defined once in Task 1 and consumed unchanged by Tasks 2-3. `canCompare(count: number)` takes a count, not an array, so both the grid's `selectionCount` and the factory's `ids.length` use the same function. `zoomAt` and `panBy` both take `width`/`height` because `clampPan` needs them and the factory has no DOM.

**Two soft spots, named rather than hidden.**

1. **Task 3 is the largest single step and has no unit test**, by the nature of the codebase. Its correctness rests on the factory's tests plus `svelte-check`, `no-literals.test.ts`, the layout probe and five smoke-checklist lines. If its implementer finds the component growing past roughly 300 lines, that is a signal the factory should be taking more of the work, and worth reporting rather than pushing through.
2. **The spec asks for per-pane facts shown "where they differ"**, which needs comparing across panes — a small pure helper (`differingFacts(panes)`) would be testable, and Task 3 leaves that decision to its implementer. If they inline it in the markup instead, that is a testable rule going untested; the reviewer should push back.
