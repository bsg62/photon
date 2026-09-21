# Grid Thumbnail Size Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user choose how large the grid draws its tiles — Small (120 px), Medium (160 px, today's grid and the default) or Large (224 px) — from the top bar and from Settings, remembered across restarts.

**Architecture:** `TILE` stops being a module constant in `ui/src/lib/layout.ts` and becomes an argument threaded through the three pure functions that read it. The choice is stored in the existing `settings` key/value table as a named step (not a pixel width), reaches the UI over a new IPC pair modelled exactly on `slideshow_interval`, and is held in one `createGridSize` store so the two controls cannot disagree. Changing the size re-pins the grid's scroll position to the photo that was at the top.

**Tech Stack:** Rust (photon-core `library/settings.rs`, photon-app `commands.rs`/`ipc.rs`/`app.rs`), Svelte 5 runes + TypeScript, vitest, `xtask screenshots`.

**Spec:** `docs/superpowers/specs/2026-09-21-photon-thumbnail-size-design.md`

## Global Constraints

- **No schema migration.** The `settings` table is key/value TEXT and already exists. `library/mod.rs`'s hardcoded schema version assertions and table count must **not** change.
- **Every step stays at or below 256 px.** `ThumbSize::Grid` renders a 256 px maximum edge into a cache directory named `grid`; raising `max_edge()` without renaming the directory would silently reuse cached 256 px files at the new size. Do not touch `crates/photon-core/src/thumbs/cache.rs` in this work.
- **The three widths are exactly `Small = 120`, `Medium = 160`, `Large = 224`.** Medium is the default and is today's value.
- **The stored value is the step name** (`"small"` / `"medium"` / `"large"`), never a pixel width.
- **The Rust gate, all four, before any commit:** `cargo fmt --all` (run it, not just `--check`), then `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`.
- **The UI gate:** `npm run check` (svelte-check, **0 errors and 0 warnings**) and `npm test`, both from the repo root.
- **IPC is three files plus the mirror.** A command means `commands.rs`, then `ipc.rs`, then an entry in `app.rs`'s `tauri::generate_handler![...]` — forgetting the third compiles fine and fails at runtime — plus the hand-written TypeScript mirror in `ui/src/lib/api.ts`, which nothing validates, plus an answer in `crates/xtask/screenshots/mock.js` or the test in `screenshots.rs` fails.
- **Never launch the GUI to verify.** Verification is the test suites plus `svelte-check`; anything needing eyes goes on the README's `## Manual smoke checklist`.
- **A new test must be demonstrated to fail with its change reverted.** A compile error is not proof. Where a change genuinely cannot have a discriminating test, say so in the commit message and why.
- **Branch:** create `feat/grid-tile-size` off `main` before Task 1. Do not work on `docs/four-feature-specs`.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/photon-core/src/library/settings.rs` | `GridTile` enum, `grid_tile()` / `set_grid_tile()` | 1 |
| `crates/photon-core/src/library/mod.rs` | re-export `GridTile` | 1 |
| `crates/photon-app/src/commands.rs` | `grid_tile` / `set_grid_tile` command logic | 2 |
| `crates/photon-app/src/ipc.rs` | delegating `#[tauri::command(async)]` wrappers | 2 |
| `crates/photon-app/src/app.rs` | two `generate_handler!` entries | 2 |
| `ui/src/lib/api.ts` | `GridTile` type + two calls (hand-written mirror) | 2 |
| `crates/xtask/screenshots/mock.js` | canned answers for both commands | 2 |
| `ui/src/lib/layout.ts` | tile size as an argument; scroll re-pin helpers | 3 |
| `ui/src/lib/layout.test.ts` | the same rules at 120 and 224 | 3 |
| `ui/src/tokens.css` | scope the `[tabindex='-1']:focus-visible` rule | 4 |
| `ui/src/lib/grid-size.svelte.ts` | `createGridSize` — pure, injected deps | 5 |
| `ui/src/lib/grid-size.svelte.test.ts` | its tests | 5 |
| `ui/src/lib/app-grid-size.svelte.ts` | the singleton, wired to `api` | 5 |
| `ui/src/components/Grid.svelte` | reads the size; re-pins on change | 6 |
| `ui/src/components/Tile.svelte` | tile width/height from a prop | 6 |
| `ui/src/components/SizeControl.svelte` | the segmented control | 7 |
| `ui/src/App.svelte` | the control in the top bar | 7 |
| `ui/src/components/Settings.svelte` | the same control under Appearance | 7 |
| `crates/xtask/src/screenshots.rs` | `grid-small` / `grid-large` shots | 8 |
| `README.md` | smoke checklist entries | 8 |

---

### Task 1: `GridTile` in the library

**Files:**
- Modify: `crates/photon-core/src/library/settings.rs`
- Modify: `crates/photon-core/src/library/mod.rs:17`
- Test: `crates/photon-core/src/library/settings.rs` (the `#[cfg(test)] mod tests` already at the bottom of that file)

**Interfaces:**
- Consumes: nothing.
- Produces: `photon_core::library::GridTile` (`Small | Medium | Large`, serde `rename_all = "lowercase"`, `Default` = `Medium`); `Library::grid_tile(&self) -> Result<GridTile>`; `Library::set_grid_tile(&self, tile: GridTile) -> Result<()>`.

- [ ] **Step 1: Create the branch**

```bash
git checkout main
git checkout -b feat/grid-tile-size
```

- [ ] **Step 2: Write the failing tests**

Add to the `mod tests` block at the bottom of `crates/photon-core/src/library/settings.rs`. Look at the existing tests there first and match how they obtain a `Library` (the file already uses `temp_library()` from `crate::testutil`).

```rust
    #[test]
    fn grid_tile_defaults_to_medium() {
        let (_dir, lib) = temp_library();
        assert_eq!(lib.grid_tile().unwrap(), GridTile::Medium);
    }

    #[test]
    fn grid_tile_round_trips() {
        let (_dir, lib) = temp_library();
        for tile in [GridTile::Small, GridTile::Large, GridTile::Medium] {
            lib.set_grid_tile(tile).unwrap();
            assert_eq!(lib.grid_tile().unwrap(), tile, "{tile:?}");
        }
    }

    /// The table is plain text an older or newer photon may have written, so an
    /// unrecognised step falls back rather than erroring - the same rule
    /// `ThemeChoice::parse` follows for an unknown theme.
    #[test]
    fn an_unknown_grid_tile_falls_back_to_medium() {
        let (_dir, lib) = temp_library();
        lib.set_setting(GRID_TILE, "enormous").unwrap();
        assert_eq!(lib.grid_tile().unwrap(), GridTile::Medium);
    }
```

If `temp_library()` is not already imported in that test module, add it to the module's `use super::*;` neighbours the same way the existing tests do.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p photon-core grid_tile`
Expected: FAIL to compile — `cannot find type GridTile in this scope`, `no method named grid_tile`.

- [ ] **Step 4: Write the implementation**

In `crates/photon-core/src/library/settings.rs`, beside the other key constants near the top (after `EXPORT_APPLY_EDITS` at line 30):

```rust
/// How large the grid draws its tiles. Stored as the step's name rather than its pixel
/// width, so changing what "Large" measures does not have to migrate anyone's setting.
const GRID_TILE: &str = "grid_tile";
```

After the `ThemeChoice` enum and its `impl` block:

```rust
/// How large the grid draws its tiles. The widths themselves live in the UI
/// (`ui/src/lib/layout.ts`), because they are a layout fact, not a stored one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GridTile {
    Small,
    #[default]
    Medium,
    Large,
}

impl GridTile {
    fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    fn parse(stored: &str) -> Self {
        match stored {
            "small" => Self::Small,
            "large" => Self::Large,
            _ => Self::Medium,
        }
    }
}
```

In `impl Library`, immediately after `set_theme` (line 183):

```rust
    /// How large the grid draws its tiles; `Medium` when never set.
    pub fn grid_tile(&self) -> Result<GridTile> {
        Ok(self
            .setting(GRID_TILE)?
            .map_or(GridTile::Medium, |stored| GridTile::parse(&stored)))
    }

    /// Stores the grid's tile size.
    pub fn set_grid_tile(&self, tile: GridTile) -> Result<()> {
        self.set_setting(GRID_TILE, tile.as_str())
    }
```

In `crates/photon-core/src/library/mod.rs:17`, extend the re-export:

```rust
pub use settings::{GridTile, ThemeChoice};
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p photon-core grid_tile`
Expected: PASS, 3 tests.

- [ ] **Step 6: Prove the fallback test discriminates**

Temporarily change `GridTile::parse`'s `_ => Self::Medium` arm to `_ => Self::Large`.
Run: `cargo test -p photon-core grid_tile`
Expected: `an_unknown_grid_tile_falls_back_to_medium` FAILS. Restore the arm exactly (edit it back by hand; do not use a loose `sed`) and re-run to confirm PASS.

- [ ] **Step 7: Run the Rust gate**

```bash
cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add crates/photon-core/src/library/settings.rs crates/photon-core/src/library/mod.rs
git commit -m "feat(core): remember the grid's tile size

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: The IPC pair and its TypeScript mirror

**Files:**
- Modify: `crates/photon-app/src/commands.rs` (after `set_theme`, ~line 409)
- Modify: `crates/photon-app/src/ipc.rs` (after the `set_theme` wrapper)
- Modify: `crates/photon-app/src/app.rs:202` (the `generate_handler!` list)
- Modify: `ui/src/lib/api.ts` (type near line 23, calls near line 161)
- Modify: `crates/xtask/screenshots/mock.js` (the `canned` object)

**Interfaces:**
- Consumes: `photon_core::library::GridTile`, `Library::grid_tile`, `Library::set_grid_tile` from Task 1.
- Produces: Tauri commands `grid_tile` (no args, returns `GridTile`) and `set_grid_tile` (arg `tile`, returns nothing); TypeScript `export type GridTile = 'small' | 'medium' | 'large'`, `api.gridTile()`, `api.setGridTile(tile)`.

- [ ] **Step 1: Write the failing test**

There is a test in `crates/xtask/src/screenshots.rs` that reads `ui/src/lib/api.ts` and fails when a command has no answer in `mock.js`. It is the test for this task and it needs no editing — adding the commands to `api.ts` without adding them to `mock.js` must make it fail.

Do this step by adding **only** the `api.ts` half:

In `ui/src/lib/api.ts`, after the `ThemeChoice` type at line 23:

```typescript
/** Mirrors `photon_core::library::GridTile` (serde lowercase). */
export type GridTile = 'small' | 'medium' | 'large';
```

and after `setTheme` at line 161:

```typescript
  gridTile: () => invoke<GridTile>('grid_tile'),
  setGridTile: (tile: GridTile) => invoke<void>('set_grid_tile', { tile }),
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p xtask`
Expected: FAIL — the mock-coverage test names `grid_tile` and `set_grid_tile` as commands with no answer.

- [ ] **Step 3: Write the Rust side**

`crates/photon-app/src/commands.rs` — add `GridTile` to the `photon_core::library::{...}` import list at line 10, then after `set_theme`:

```rust
/// How large the grid draws its tiles.
pub fn grid_tile(engine: &Engine) -> CmdResult<GridTile> {
    Ok(engine.lib.grid_tile()?)
}

pub fn set_grid_tile(engine: &Engine, tile: GridTile) -> CmdResult<()> {
    Ok(engine.lib.set_grid_tile(tile)?)
}
```

`crates/photon-app/src/ipc.rs` — after the `set_theme` wrapper:

```rust
#[tauri::command(async)]
pub fn grid_tile(engine: Eng<'_>) -> Result<photon_core::library::GridTile, AppError> {
    commands::grid_tile(&engine)
}

#[tauri::command(async)]
pub fn set_grid_tile(
    engine: Eng<'_>,
    tile: photon_core::library::GridTile,
) -> Result<(), AppError> {
    commands::set_grid_tile(&engine, tile)
}
```

`crates/photon-app/src/app.rs` — after `ipc::set_theme,` at line 202:

```rust
            ipc::grid_tile,
            ipc::set_grid_tile,
```

- [ ] **Step 4: Write the mock answers**

In `crates/xtask/screenshots/mock.js`, inside the `canned` object (keys at four spaces' indent, as the file's header comment says):

```javascript
    grid_tile: () => P.get('tile') || 'medium',
    set_grid_tile: () => null,
```

The `tile` query parameter is what Task 8's screenshots will vary.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p xtask && cargo test --workspace && npm run check`
Expected: all PASS, and `npm run check` reports 0 errors and 0 warnings.

- [ ] **Step 6: Prove the mock test discriminates**

Temporarily delete the two lines added to `mock.js` in Step 4.
Run: `cargo test -p xtask`
Expected: FAIL naming both commands. Restore the two lines and re-run to confirm PASS.

- [ ] **Step 7: Run the Rust gate**

```bash
cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```

- [ ] **Step 8: Commit**

```bash
git add crates/photon-app/src/commands.rs crates/photon-app/src/ipc.rs crates/photon-app/src/app.rs ui/src/lib/api.ts crates/xtask/screenshots/mock.js
git commit -m "feat(app): expose the grid tile size over IPC

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Tile size as an argument in `layout.ts`

This is the task with the real risk in it. `itemsInRect` is the rubber band's selection rule, and CLAUDE.md records that six passing probes on it left four of its rules undefended because each probe only exercised inputs the existing tests happened to cover. Every existing `itemsInRect` test therefore gains a Small and a Large case.

**Files:**
- Modify: `ui/src/lib/layout.ts`
- Modify: `ui/src/lib/layout.test.ts`
- Modify: `ui/src/lib/timeline.test.ts:18,31,36` (its `buildRows` calls)

**Interfaces:**
- Consumes: nothing.
- Produces, all exported from `ui/src/lib/layout.ts`:
  - `export type TileSize = 'small' | 'medium' | 'large'`
  - `export const TILE_WIDTH: Record<TileSize, number>` = `{ small: 120, medium: 160, large: 224 }`
  - `export function tileRow(tile: number): number` — `tile + GAP`
  - `columnsFor(width: number, tile = TILE_WIDTH.medium): number`
  - `buildRows(sections: SectionLike[], columns: number, headers = true, tile = TILE_WIDTH.medium): Row[]`
  - `itemsInRect(rows: Row[], rect: Rect, tile = TILE_WIDTH.medium): [number, number][]`
  - `firstVisibleOffset(rows: Row[], scrollTop: number): number | null`
  - `scrollTopForOffset(rows: Row[], offset: number): number | null`
  - `TILE` is **removed**; `TILE_ROW` is **removed** (replaced by `tileRow`).

The defaults exist so the many existing call sites in tests keep working unchanged; production call sites pass the real size.

- [ ] **Step 1: Write the failing tests**

In `ui/src/lib/layout.test.ts`, change the import line to drop `TILE` and `TILE_ROW` and add the new names:

```typescript
import { buildRows, columnsFor, edgeScrollSpeed, firstVisibleOffset, GAP, HEADER, itemSpan, itemsInRect, layoutSections, rowIndexAt, rowOfItem, scrollTopForOffset, tileRow, TILE_WIDTH, topFolderId, totalHeight, visibleRange } from './layout';
```

Then, wherever the existing tests use `TILE` or `TILE_ROW`, replace them with `TILE_WIDTH.medium` and `tileRow(TILE_WIDTH.medium)` — the numbers do not change, so those tests must keep passing untouched in meaning.

Add these new tests:

```typescript
describe('tile size', () => {
  it('columns depend on the tile size', () => {
    // 800px of canvas: 168px rows at medium, 128 at small, 232 at large.
    expect(columnsFor(800, TILE_WIDTH.medium)).toBe(4);
    expect(columnsFor(800, TILE_WIDTH.small)).toBe(6);
    expect(columnsFor(800, TILE_WIDTH.large)).toBe(3);
  });

  it('rows are taller at a larger tile size', () => {
    const sections = [{ folderId: 1, offset: 0, count: 4 }];
    const small = buildRows(sections, 2, false, TILE_WIDTH.small);
    const large = buildRows(sections, 2, false, TILE_WIDTH.large);
    expect(small[1].top).toBe(tileRow(TILE_WIDTH.small));
    expect(large[1].top).toBe(tileRow(TILE_WIDTH.large));
  });
});

describe('itemsInRect at other tile sizes', () => {
  // The same shape as the medium-size block above: one section of 5 photos, 3 columns,
  // no headers - re-run at both ends, because a band rule pinned only at 160 is pinned
  // by the one size where a hardcoded 160 would still be right.
  for (const size of ['small', 'large'] as const) {
    const tile = TILE_WIDTH[size];
    const row = tileRow(tile);
    const rows = buildRows([{ folderId: 1, offset: 0, count: 5 }], 3, false, tile);
    const rect = (x0: number, y0: number, x1: number, y1: number) => ({ x0, y0, x1, y1 });

    it(`${size}: a band over a whole row takes the row`, () => {
      expect(itemsInRect(rows, rect(0, 0, 1000, 1000), tile)).toEqual([[0, 4]]);
    });

    it(`${size}: a band starting in the gap after tile 0 starts at tile 1`, () => {
      expect(itemsInRect(rows, rect(GAP + tile + 2, 10, 1000, 20), tile)).toEqual([[1, 2]]);
    });

    it(`${size}: a band wholly inside the gap below a row selects nothing`, () => {
      expect(itemsInRect(rows, rect(0, tile + 1, 1000, row - 1), tile)).toEqual([]);
    });

    it(`${size}: a band over one column of two rows merges into one range`, () => {
      const tall = buildRows([{ folderId: 1, offset: 0, count: 9 }], 3, false, tile);
      expect(itemsInRect(tall, rect(0, 0, 1000, 2 * row), tile)).toEqual([[0, 5]]);
    });

    it(`${size}: the last short row does not run into the next section`, () => {
      expect(itemsInRect(rows, rect(0, row + 1, 1000, row + tile), tile)).toEqual([[3, 4]]);
    });
  }
});

describe('keeping your place across a size change', () => {
  const sections = [{ folderId: 1, offset: 0, count: 9 }];

  it('reports the first item of the row at the top of the viewport', () => {
    const rows = buildRows(sections, 3, false, TILE_WIDTH.medium);
    expect(firstVisibleOffset(rows, 0)).toBe(0);
    expect(firstVisibleOffset(rows, tileRow(TILE_WIDTH.medium))).toBe(3);
    expect(firstVisibleOffset([], 0)).toBeNull();
  });

  it('finds the scroll position that puts an offset back at the top', () => {
    const large = buildRows(sections, 3, false, TILE_WIDTH.large);
    expect(scrollTopForOffset(large, 3)).toBe(tileRow(TILE_WIDTH.large));
    expect(scrollTopForOffset(large, 999)).toBeNull();
  });

  it('round-trips an offset from one size to another', () => {
    const medium = buildRows(sections, 3, false, TILE_WIDTH.medium);
    const large = buildRows(sections, 2, false, TILE_WIDTH.large);
    const offset = firstVisibleOffset(medium, 2 * tileRow(TILE_WIDTH.medium));
    expect(offset).toBe(6);
    // Row 3 of the two-column large layout holds offsets 6 and 7.
    expect(scrollTopForOffset(large, offset!)).toBe(3 * tileRow(TILE_WIDTH.large));
  });
});
```

Also update `ui/src/lib/timeline.test.ts` — its three `buildRows` calls already omit the tile argument, so they need no change, but the file must still typecheck; run `npm run check` in Step 5 to confirm.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npm test -w ui -- src/lib/layout.test.ts`
Expected: FAIL — `TILE_WIDTH`, `tileRow`, `firstVisibleOffset` and `scrollTopForOffset` are not exported.

- [ ] **Step 3: Write the implementation**

In `ui/src/lib/layout.ts`, replace the `TILE` / `TILE_ROW` constants:

```typescript
export type TileSize = 'small' | 'medium' | 'large';

/** How wide a tile is at each step, in CSS pixels.
 *
 *  Every step is at or below 256, which is `ThumbSize::Grid`'s maximum edge. The cache key
 *  is the photo's fingerprint while the cache *directory* is what separates the sizes, so
 *  raising that maximum without renaming the directory would silently serve already-cached
 *  256px files at the new size - soft forever, with nothing to say why. A step above 256
 *  needs its own `ThumbSize` variant, not a larger `Grid`. */
export const TILE_WIDTH: Record<TileSize, number> = { small: 120, medium: 160, large: 224 };

export const GAP = 8;
export const HEADER = 32;

/** A tile row's full height: the tile plus the gutter under it. */
export function tileRow(tile: number): number {
  return tile + GAP;
}
```

(`GAP` and `HEADER` keep their existing values and positions; `TILE` and `TILE_ROW` are removed.)

Then thread the argument through the three functions:

```typescript
export function columnsFor(width: number, tile: number = TILE_WIDTH.medium): number {
  return Math.max(1, Math.floor((width + GAP) / tileRow(tile)));
}
```

```typescript
export function buildRows(
  sections: SectionLike[],
  columns: number,
  headers = true,
  tile: number = TILE_WIDTH.medium,
): Row[] {
  const rows: Row[] = [];
  let top = 0;
  sections.forEach((s, section) => {
    if (headers) {
      rows.push({ kind: 'header', section, first: s.offset, count: 0, top, height: HEADER });
      top += HEADER;
    }
    const end = s.offset + s.count;
    for (let first = s.offset; first < end; first += columns) {
      rows.push({ kind: 'tiles', section, first, count: Math.min(columns, end - first), top, height: tileRow(tile) });
      top += tileRow(tile);
    }
  });
  return rows;
}
```

In `itemsInRect`, add the parameter and replace the two constant reads. **Keep the existing comment block verbatim** — it explains why `firstColumn` measures from tile right edges and `lastColumn` from left edges, and it is load-bearing:

```typescript
export function itemsInRect(rows: Row[], rect: Rect, tile: number = TILE_WIDTH.medium): [number, number][] {
```

and inside the loop:

```typescript
    if (row.top + tile < top || row.top > bottom) continue;
```
```typescript
    const firstColumn = Math.max(0, Math.ceil((left - GAP - tile) / tileRow(tile)));
    const lastColumn = Math.min(row.count - 1, Math.floor((right - GAP) / tileRow(tile)));
```

Add the two re-pin helpers at the end of the file:

```typescript
/** The grid offset of the first photo in the row at the top of the viewport, or null when
 *  the grid has no rows.
 *
 *  Paired with `scrollTopForOffset` to keep your place when the tile size changes: every
 *  row's `top` moves, so a scroll position kept as a number points somewhere else
 *  afterwards, and a grid that jumps to a different year when the tiles grow is worse than
 *  no size control at all. A header row answers with the offset of the section it heads,
 *  which is the photo the eye is on. */
export function firstVisibleOffset(rows: Row[], scrollTop: number): number | null {
  if (rows.length === 0) return null;
  return rows[rowIndexAt(rows, scrollTop)].first;
}

/** The scroll position that puts `offset`'s row at the top of the viewport, or null when
 *  no row holds it. */
export function scrollTopForOffset(rows: Row[], offset: number): number | null {
  const row = rowOfItem(rows, offset);
  return row < 0 ? null : rows[row].top;
}
```

- [ ] **Step 4: Fix the existing call sites**

`ui/src/components/Tile.svelte:4` imports `TILE`, and `ui/src/components/Grid.svelte:4` imports from `layout`. Neither is rewired in this task — Task 6 does that — but both must keep compiling. For now, in `Tile.svelte` change the import to `TILE_WIDTH` and the two `style:` bindings to `{TILE_WIDTH.medium}px`, with a `// Task 6 replaces this with a prop.` comment; `Grid.svelte` needs no change, because `columnsFor`, `buildRows` and `itemsInRect` all default to medium.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `npm test -w ui && npm run check`
Expected: all PASS; `npm run check` reports 0 errors and 0 warnings.

- [ ] **Step 6: Prove the new band tests discriminate**

Temporarily change `itemsInRect`'s `firstColumn` line back to a hardcoded medium:

```typescript
    const firstColumn = Math.max(0, Math.ceil((left - GAP - 160) / 168));
```

Run: `npm test -w ui -- src/lib/layout.test.ts`
Expected: the `small:` and `large:` band cases FAIL while every medium-size case still PASSES — which is the whole point of adding them. Restore the line by hand and re-run to confirm PASS.

- [ ] **Step 7: Commit**

```bash
git add ui/src/lib/layout.ts ui/src/lib/layout.test.ts ui/src/components/Tile.svelte
git commit -m "refactor(ui): make the grid's tile size an argument

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Scope the focus-ring suppression

The global `[tabindex='-1']:focus-visible { outline: none }` rule exists for script-focused containers — the viewer, the settings dialog, an open menu. Task 7 adds the first roving-tabindex widget in this codebase, whose items also carry `tabindex="-1"`, and they would silently lose their focus ring. CLAUDE.md names this trap; this task closes it **before** the widget exists.

**Files:**
- Modify: `ui/src/tokens.css:100-102`
- Test: `ui/src/lib/no-literals.test.ts` and `ui/src/lib/tokens.test.ts` must both still pass (they read `tokens.css`).

**Interfaces:**
- Consumes: nothing.
- Produces: the CSS class `.focus-container`, which any script-focused container must carry to keep suppressing its ring.

- [ ] **Step 1: Find every element that relies on the rule today**

Run: `grep -rn 'tabindex="-1"' ui/src --include=*.svelte`
Record the list. Every one of them is either a script-focused container (which must gain `class="focus-container"`) or a tile (which must **not** — the grid's tiles draw selection with `.selected`, not with focus, and they are not focused by script).

- [ ] **Step 2: Write the failing test**

Add to `ui/src/lib/tokens.test.ts`:

```typescript
it('the focus-ring suppression is scoped to containers, not to every tabindex=-1', () => {
  // A roving-tabindex widget's items carry tabindex="-1" too. An unscoped rule strips
  // their focus ring and the keyboard user has no idea where they are.
  expect(css).not.toMatch(/^\[tabindex='-1'\]:focus-visible/m);
  expect(css).toMatch(/\.focus-container\[tabindex='-1'\]:focus-visible/);
});
```

`css` is already in scope: `tokens.test.ts` imports the stylesheet at the top as
`import css from '../tokens.css?raw';`. Use that binding rather than reading the file again.

- [ ] **Step 3: Run the test to verify it fails**

Run: `npm test -w ui -- src/lib/tokens.test.ts`
Expected: FAIL on both assertions.

- [ ] **Step 4: Write the implementation**

In `ui/src/tokens.css`, replace lines 98-102:

```css
/* Containers focused from script - the viewer, the settings dialog, an open menu - so that
   keys reach them. A ring around the whole window says nothing.

   Scoped to `.focus-container` rather than to every `tabindex='-1'`: a roving-tabindex
   widget's items carry that attribute too, and an unscoped rule takes their focus ring
   away, which leaves a keyboard user with no indication of where they are. */
.focus-container[tabindex='-1']:focus-visible {
  outline: none;
}
```

Then add `class="focus-container"` (or append it to the existing class list) to every script-focused container found in Step 1. Do **not** add it to `Tile.svelte`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `npm test -w ui && npm run check`
Expected: all PASS, 0 errors and 0 warnings.

- [ ] **Step 6: Prove the test discriminates**

Temporarily change the selector back to `[tabindex='-1']:focus-visible`.
Run: `npm test -w ui -- src/lib/tokens.test.ts`
Expected: FAIL. Restore and confirm PASS.

- [ ] **Step 7: Note for the smoke checklist**

Whether the containers still have no ring, and whether the tiles still have none either, cannot be tested here. Write both lines down now; Task 8 adds them to the README.

- [ ] **Step 8: Commit**

```bash
git add ui/src/tokens.css ui/src/lib/tokens.test.ts ui/src/components
git commit -m "fix(ui): scope the focus-ring suppression to script-focused containers

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: The `createGridSize` store

**Files:**
- Create: `ui/src/lib/grid-size.svelte.ts`
- Create: `ui/src/lib/grid-size.svelte.test.ts`
- Create: `ui/src/lib/app-grid-size.svelte.ts`

**Interfaces:**
- Consumes: `TileSize` and `TILE_WIDTH` from `./layout` (Task 3); `GridTile` and `api.gridTile`/`api.setGridTile` from `./api` (Task 2).
- Produces: `createGridSize(deps: GridSizeDeps)` returning `{ get size(): TileSize, get width(): number, init(): Promise<void>, set(next: TileSize): Promise<void>, dispose(): void }`; and the singleton `gridSize` exported from `./app-grid-size.svelte`.

- [ ] **Step 1: Write the failing tests**

Create `ui/src/lib/grid-size.svelte.test.ts`:

```typescript
import { describe, expect, it, vi } from 'vitest';
import { createGridSize } from './grid-size.svelte';
import { TILE_WIDTH } from './layout';

function deps(overrides: Partial<Parameters<typeof createGridSize>[0]> = {}) {
  return {
    load: vi.fn(async () => 'medium' as const),
    save: vi.fn(async () => {}),
    onerror: vi.fn(),
    ...overrides,
  };
}

describe('createGridSize', () => {
  it('starts at medium before anything has loaded', () => {
    const size = createGridSize(deps());
    expect(size.size).toBe('medium');
    expect(size.width).toBe(TILE_WIDTH.medium);
  });

  it('adopts the stored size', async () => {
    const size = createGridSize(deps({ load: vi.fn(async () => 'large' as const) }));
    await size.init();
    expect(size.size).toBe('large');
    expect(size.width).toBe(TILE_WIDTH.large);
  });

  it('applies first and saves second', async () => {
    const d = deps();
    const size = createGridSize(d);
    await size.init();
    const pending = size.set('small');
    // The click is answered before the write lands.
    expect(size.size).toBe('small');
    await pending;
    expect(d.save).toHaveBeenCalledWith('small');
  });

  it('keeps the size for this session when the save fails', async () => {
    const d = deps({ save: vi.fn(async () => { throw new Error('disk'); }) });
    const size = createGridSize(d);
    await size.init();
    await size.set('large');
    expect(size.size).toBe('large');
    expect(d.onerror).toHaveBeenCalled();
  });

  // The generation counter, as `createTheme` has: the singleton outlives an App remount.
  it('a load that lands after dispose does not change the size', async () => {
    let release!: (value: 'large') => void;
    const d = deps({ load: vi.fn(() => new Promise<'large'>((r) => (release = r))) });
    const size = createGridSize(d);
    const pending = size.init();
    size.dispose();
    release('large');
    await pending;
    expect(size.size).toBe('medium');
  });

  it('a choice made while the load is in flight wins', async () => {
    let release!: (value: 'large') => void;
    const d = deps({ load: vi.fn(() => new Promise<'large'>((r) => (release = r))) });
    const size = createGridSize(d);
    const pending = size.init();
    await size.set('small');
    release('large');
    await pending;
    expect(size.size).toBe('small');
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npm test -w ui -- src/lib/grid-size.svelte.test.ts`
Expected: FAIL — the module does not exist.

- [ ] **Step 3: Write the implementation**

Create `ui/src/lib/grid-size.svelte.ts`:

```typescript
import { TILE_WIDTH, type TileSize } from './layout';

export interface GridSizeDeps {
  load(): Promise<TileSize>;
  save(size: TileSize): Promise<void>;
  onerror(e: unknown): void;
}

/** How large the grid draws its tiles. One store behind both controls - the top bar's and
 *  Settings' - so they cannot disagree.
 *
 *  Generation-counted like `createTheme`, and for the same reason: in production this is a
 *  module singleton that outlives an App remount, so a load started under an earlier
 *  generation must be able to tell it is no longer current. A one-way `disposed` boolean
 *  would never let the singleton come back to life. */
export function createGridSize(deps: GridSizeDeps) {
  let size = $state<TileSize>('medium');
  let generation = 0;
  /** True once `set()` has been called since the current init's load started, so the
   *  load's eventual answer does not clobber a choice made while it was still pending. */
  let setDuringLoad = false;

  return {
    get size() {
      return size;
    },
    get width() {
      return TILE_WIDTH[size];
    },

    async init() {
      const myGeneration = ++generation;
      setDuringLoad = false;
      try {
        const loaded = await deps.load();
        if (myGeneration === generation && !setDuringLoad) size = loaded;
      } catch (e) {
        if (myGeneration === generation) deps.onerror(e);
      }
    },

    /** Applies first and saves second, so the click is answered at once. A failed save
     *  keeps the size for this session: reverting it would punish the user for a disk
     *  error with a reflow. */
    async set(next: TileSize) {
      size = next;
      setDuringLoad = true;
      try {
        await deps.save(next);
      } catch (e) {
        deps.onerror(e);
      }
    },

    dispose() {
      generation++;
    },
  };
}

export type GridSize = ReturnType<typeof createGridSize>;
```

Create `ui/src/lib/app-grid-size.svelte.ts`:

```typescript
import { api } from './api';
import { library } from './library.svelte';
import { createGridSize } from './grid-size.svelte';

/** The app's grid tile size. The logic is `createGridSize`'s; this is the wiring it is
 *  injected with, in its own module so importing `createGridSize` in a test touches no
 *  Tauri. */
export const gridSize = createGridSize({
  load: () => api.gridTile(),
  save: (size) => api.setGridTile(size),
  onerror: (e) => library.reportError(e),
});
```

Check how `app-theme.svelte.ts` reports errors (its `onerror`) and use exactly the same call rather than the `library.reportError` above if it differs.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `npm test -w ui -- src/lib/grid-size.svelte.test.ts && npm run check`
Expected: PASS; 0 errors and 0 warnings.

- [ ] **Step 5: Prove the race tests discriminate**

Temporarily remove `&& !setDuringLoad` from `init`.
Run: `npm test -w ui -- src/lib/grid-size.svelte.test.ts`
Expected: `a choice made while the load is in flight wins` FAILS. Restore it. Then temporarily change `dispose()` to a no-op and confirm `a load that lands after dispose does not change the size` FAILS. Restore it and confirm all PASS.

- [ ] **Step 6: Commit**

```bash
git add ui/src/lib/grid-size.svelte.ts ui/src/lib/grid-size.svelte.test.ts ui/src/lib/app-grid-size.svelte.ts
git commit -m "feat(ui): a store for the grid's tile size

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: Wire the grid and the tile

**Files:**
- Modify: `ui/src/components/Grid.svelte:4,42,53` and the `bandRanges` function at ~line 261
- Modify: `ui/src/components/Tile.svelte:4,90-91`
- Modify: `ui/src/App.svelte` (call `gridSize.init()` / `gridSize.dispose()` where `theme.init()` / `theme.dispose()` are already called)

**Interfaces:**
- Consumes: `gridSize` from `../lib/app-grid-size.svelte` (Task 5); `firstVisibleOffset`, `scrollTopForOffset`, `tileRow` from `../lib/layout` (Task 3).
- Produces: `Tile.svelte` gains a required `tile: number` prop.

- [ ] **Step 1: Find where the theme singleton is driven**

Run: `grep -n 'theme.init\|theme.dispose' ui/src/App.svelte`
`gridSize.init()` and `gridSize.dispose()` go in exactly the same places, so the two singletons share a lifecycle.

- [ ] **Step 2: Write the implementation — `Tile.svelte`**

Replace the `TILE_WIDTH` import added in Task 3 with a prop. In the `$props()` destructuring, add `tile`:

```svelte
  let { /* existing props */, tile }: { /* existing types */; tile: number } = $props();
```

and the two style bindings:

```svelte
  style:width="{tile}px"
  style:height="{tile}px"
```

Remove the now-unused `layout` import if nothing else in the file uses it.

- [ ] **Step 3: Write the implementation — `Grid.svelte`**

Import the store and the two helpers:

```svelte
  import { buildRows, columnsFor, edgeScrollSpeed, firstVisibleOffset, GAP, itemSpan, itemsInRect, layoutSections, type Rect, rowOfItem, scrollTopForOffset, topFolderId, totalHeight, visibleRange } from '../lib/layout';
  import { gridSize } from '../lib/app-grid-size.svelte';
```

Thread the width through the three derivations (line 42 and 53):

```svelte
  const columns = $derived(columnsFor(Math.max(0, width - 2 * GAP), gridSize.width));
```
```svelte
  const rows = $derived(buildRows(sections, columns, headers, gridSize.width));
```

and through `bandRanges`:

```svelte
  function bandRanges(rect: Rect): [number, number][] {
    return itemsInRect(rows, rect, gridSize.width);
  }
```

Pass the width to each `<Tile>` — find the `<Tile ... />` usage and add `tile={gridSize.width}`.

- [ ] **Step 4: Write the implementation — keeping your place**

Add to `Grid.svelte`, near the other effects:

```svelte
  /** The tile size changed, so every row's `top` moved. Put the photo that was at the top
   *  of the viewport back at the top of the viewport.
   *
   *  The grid index has not moved, so this is not the "an offset is only meaningful
   *  against one index version" hazard - but what the user sees is the same either way: a
   *  grid that jumps to a different year when the tiles grow. Read before the layout
   *  changes, applied after. */
  let pinned: number | null = null;
  $effect(() => {
    // Read the size so this runs when it changes, and read the offset from the layout as
    // it was a moment ago.
    gridSize.width;
    pinned = firstVisibleOffset(rows, scrollTop);
  });
```

Then, after `rows` has been rebuilt, scroll back. Match how the file already performs post-layout scrolls (it scrolls a row flush to the top for ArrowUp, Home and Recent's first row) and reuse that mechanism rather than inventing a second one; the value to scroll to is `scrollTopForOffset(rows, pinned)`, and a `null` answer means do nothing.

Read the surrounding effects before writing this: the ordering between the read and the write is the whole correctness of it, and it depends on how the file already sequences `$effect` against `$derived`.

- [ ] **Step 5: Wire the singleton's lifecycle**

In `ui/src/App.svelte`, beside the existing `theme.init()` and `theme.dispose()` calls found in Step 1:

```typescript
  void gridSize.init();
```
```typescript
  gridSize.dispose();
```

with `import { gridSize } from './lib/app-grid-size.svelte';` at the top.

- [ ] **Step 6: Run the gates**

Run: `npm run check && npm test -w ui`
Expected: 0 errors, 0 warnings; all tests PASS.

- [ ] **Step 7: Record what has no test**

The re-pin effect is effect wiring in a `.svelte` file, and there is no component test harness here — `vitest` runs with `environment: 'node'`, so `Grid.svelte` cannot be rendered. The *decision* it makes is pinned by `firstVisibleOffset` / `scrollTopForOffset` in Task 3; the *wiring* is not, and cannot be without a new dependency. Say exactly that in the commit message, as CLAUDE.md requires, and add the behaviour to the smoke checklist in Task 8.

- [ ] **Step 8: Commit**

```bash
git add ui/src/components/Grid.svelte ui/src/components/Tile.svelte ui/src/App.svelte
git commit -m "feat(ui): draw the grid at the chosen tile size

Keeps the photo at the top of the viewport in place across a size change.
The effect wiring has no test - vitest runs under node and cannot render a
.svelte file - so the decision it makes is pinned by firstVisibleOffset and
scrollTopForOffset in layout.test.ts, and the wiring goes on the README's
smoke checklist.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: The control, in the top bar and in Settings

**Files:**
- Create: `ui/src/components/SizeControl.svelte`
- Modify: `ui/src/App.svelte:229-233` (the `.topbar` div)
- Modify: `ui/src/components/Settings.svelte:342-359` (the Appearance section)

**Interfaces:**
- Consumes: `gridSize` from `../lib/app-grid-size.svelte` (Task 5).
- Produces: `<SizeControl />`, taking no props.

- [ ] **Step 1: Read the existing segmented control**

Open `ui/src/components/Settings.svelte:342-359`. The theme control there is `role="group"` with `aria-pressed` on independent buttons, **not** an APG radiogroup — its comment explains why: `role="radio"` without roving-tabindex key handling lies to a screen reader. Follow that same pattern exactly; do not introduce a radiogroup. That also means the items are ordinary Tab stops and do **not** carry `tabindex="-1"` — Task 4's scoping still matters, because the next widget might, and because the rule was wrong regardless.

- [ ] **Step 2: Write the component**

Create `ui/src/components/SizeControl.svelte`:

```svelte
<script lang="ts">
  import { gridSize } from '../lib/app-grid-size.svelte';
  import type { TileSize } from '../lib/layout';

  const SIZES: { value: TileSize; label: string }[] = [
    { value: 'small', label: 'Small' },
    { value: 'medium', label: 'Medium' },
    { value: 'large', label: 'Large' },
  ];
</script>

<!-- A group of independent toggle buttons, each its own Tab stop with Enter/Space to
     activate - the same pattern as the theme control in Settings, and not the APG
     radiogroup pattern, because role="radio" without roving-tabindex key handling lies to
     a screen reader. -->
<div class="segmented" role="group" aria-label="Photo size">
  {#each SIZES as option (option.value)}
    <button
      aria-pressed={gridSize.size === option.value}
      class:checked={gridSize.size === option.value}
      onclick={() => gridSize.set(option.value)}
    >
      {option.label}
    </button>
  {/each}
</div>

<style>
  /* Copy the `.segmented` rules from Settings.svelte verbatim. Every colour must be a
     `var(--token)` already declared in tokens.css: no-literals.test.ts fails on a named
     colour, a colour function used as a component colour, and any var() that tokens.css
     does not declare. */
</style>
```

Copy the `.segmented` CSS from `Settings.svelte` into the `<style>` block rather than writing new rules, so the two controls look identical.

- [ ] **Step 3: Place it in the top bar**

In `ui/src/App.svelte`, inside `.topbar`, between `<SearchBar />` and the gear button:

```svelte
    <SizeControl />
```

with `import SizeControl from './components/SizeControl.svelte';` beside the other component imports. It is inside `.topbar`, which already carries `inert={covered}`, so it is correctly disabled while any overlay is up — nothing extra is needed for that.

- [ ] **Step 4: Place it in Settings**

In `ui/src/components/Settings.svelte`, in the `appearance` branch, after the theme's `.segmented` group:

```svelte
          <h3>Photo size</h3>
          <p class="hint">How large the grid draws each photo.</p>
          <SizeControl />
```

with the import added. Match the heading level and hint markup the section already uses — if it has no `<h3>` pattern, follow whatever it does use rather than introducing a new one.

- [ ] **Step 5: Run the gates**

Run: `npm run check && npm test -w ui`
Expected: 0 errors, 0 warnings; all tests PASS — including `no-literals.test.ts`, which fails on a colour literal, a glyph icon, or a `var(--x)` that `tokens.css` does not declare.

- [ ] **Step 6: Record what has no test**

The component is markup and effect wiring; there is no component harness. Say so in the commit message.

- [ ] **Step 7: Commit**

```bash
git add ui/src/components/SizeControl.svelte ui/src/App.svelte ui/src/components/Settings.svelte
git commit -m "feat(ui): a size control in the top bar and in Settings

Both read one store, so they cannot disagree. No test: a .svelte component
cannot be rendered under vitest's node environment, so this is covered by
svelte-check, no-literals.test.ts and the smoke checklist.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: Screenshots, README and the smoke checklist

**Files:**
- Modify: `crates/xtask/src/screenshots.rs:40` (the `SHOTS` array)
- Modify: `README.md` (the `## Manual smoke checklist` section)

**Interfaces:**
- Consumes: the `tile` query parameter answered by `mock.js` in Task 2.
- Produces: two new PNGs, `grid-small` and `grid-large`.

- [ ] **Step 1: Add the shots**

In `crates/xtask/src/screenshots.rs`, in `SHOTS`, after the `main-light` / `main-dark` pair:

```rust
    // Whether 120 and 224 are the right widths is a looking question, and this is how it
    // is looked at without launching the app.
    Shot {
        name: "grid-small",
        query: "theme=light&tile=small",
        dark: false,
    },
    Shot {
        name: "grid-large",
        query: "theme=light&tile=large",
        dark: false,
    },
```

- [ ] **Step 2: Run the Rust gate**

```bash
cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```
Expected: all pass. Note the shot count in `screenshots.rs`'s own tests or docs if any of them assert a number — the README and CLAUDE.md both say "fourteen PNGs", which becomes sixteen.

- [ ] **Step 3: Generate the screenshots and look at them**

Run: `cargo run -p xtask -- screenshots --only grid-small` and `--only grid-large`
(Needs Chromium on `PATH` or in `CHROMIUM`. This is not run in CI.)
Expected: PNGs in `target/screenshots/`. Read them with the Read tool and check the tiles are visibly smaller and larger than `main-light`'s, and that nothing overflows or crowds.

- [ ] **Step 4: Update the counts in prose**

`README.md` and `CLAUDE.md` both say `screenshots` writes "fourteen PNGs". Change both to "sixteen".

- [ ] **Step 5: Add the smoke checklist entries**

In `README.md`'s `## Manual smoke checklist`, add:

```markdown
- Change the photo size in the top bar with the grid scrolled into the middle of a folder.
  The photo that was at the top of the screen should still be at the top afterwards — not
  a different year, and not the top of the library.
- Change it from Settings too, and check the top bar's control moved with it.
- Tab to the size control and press Enter. The focused button must show a focus ring.
- Open the viewer and press Escape; the arrow keys must still move the grid. Then open
  Settings and close it, and check the same. (The focus-ring rule was scoped in this work,
  so the script-focused containers are worth re-checking.)
- At Small, drag a rubber band across two rows and check it selects what it covers.
  Repeat at Large.
- Restart photon and check the size is the one you left it at.
```

- [ ] **Step 6: Commit**

```bash
git add crates/xtask/src/screenshots.rs README.md CLAUDE.md
git commit -m "docs: screenshots and smoke checks for the grid tile size

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 7: Run every gate one last time**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
npm run check
npm test
cargo run -p xtask -- versions
cargo run -p xtask -- metadata
```
Expected: all pass, `npm run check` with 0 errors and 0 warnings.

- [ ] **Step 8: Open the pull request**

```bash
git push -u origin feat/grid-tile-size
gh pr create --title "feat: choose how large the grid draws photos" --body "$(cat <<'EOF'
Small (120px), Medium (160px, today's grid and the default) or Large (224px),
from the top bar or from Settings, remembered in the `settings` table.

Spec: `docs/superpowers/specs/2026-09-21-photon-thumbnail-size-design.md`
Plan: `docs/superpowers/plans/2026-09-21-grid-thumbnail-size.md`

Every step stays at or below 256px on purpose: `ThumbSize::Grid` renders a
256px maximum edge into a cache directory named `grid`, and raising that
without renaming the directory would silently serve already-cached 256px
files at the new size. A step above 256 needs its own `ThumbSize` variant.

Also scopes the global `[tabindex='-1']:focus-visible { outline: none }` rule
to `.focus-container`. It was written for script-focused containers, and it
would have taken the focus ring from any roving-tabindex widget's items.

No schema change. Not run by a person — see the smoke checklist entries added
to the README.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Then watch CI: `gh pr checks <N> --watch`. Straight after a push it can answer "no checks reported" and exit 0; wait until checks exist before trusting it.

---

## Self-Review

**Spec coverage.** Every section of the spec maps to a task: the three widths and the 256 px constraint (Task 3's `TILE_WIDTH` comment and the Global Constraints); the layout parameterisation and the band tests at both ends (Task 3); keeping your place across a size change (Tasks 3 and 6); the `settings` table and no schema change (Task 1 and the Global Constraints); the IPC quartet and the `mock.js` answer (Task 2); no `localStorage` mirror and no boot script (nothing to do — deliberately absent, and the spec says why); one store behind both controls (Task 5); the focus-ring scoping (Task 4, ahead of the control that needs it); the segmented control in both places (Task 7); the two screenshots and the smoke checklist (Task 8). The spec's four named tests all appear: `columns_for_each_size` and `rubber_band_at_small_and_large` in Task 3, `row_of_item_survives_a_size_change` as the round-trip test in Task 3, `the_setting_round_trips` in Task 1.

**Type consistency.** `TileSize` is the TypeScript union and `GridTile` is the Rust enum and the `api.ts` type mirroring it; Task 5 converts between them implicitly because the two unions are spelled the same (`'small' | 'medium' | 'large'`). That is deliberate and is why `api.gridTile()` can be handed straight to a `TileSize`. `TILE_WIDTH` is `Record<TileSize, number>` throughout; `tileRow(tile: number)` takes a width, never a step name.

**Known soft spot.** Task 6, Step 4 — the ordering between reading the pinned offset and applying the new scroll position — is the one step this plan cannot write out completely, because it depends on how `Grid.svelte` already sequences its effects against its deriveds, and that file is 664 lines. The step says to read the surrounding code first and reuse the existing scroll mechanism. If it turns out the existing mechanism cannot be reused, stop and raise it rather than adding a second one.
