# UI Reskin, PR 3 of 5: Grid — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restyle the grid side of the main window — folder headers, the empty state, tiles, the tile context menu, the timeline scrubber and toasts — in the new component language, on tokens only, with icons in place of glyphs.

**Architecture:** CSS and markup only, in four components. No logic, handler, DOM-order or geometry change: `TILE` (160), `GAP` (8) and `HEADER` (32) in `ui/src/lib/layout.ts` fix every row's offset, so the restyle works inside those boxes.

**Tech Stack:** Svelte 5 runes, scoped component CSS, the tokens and `Icon.svelte` from PR 1.

**Spec:** `docs/superpowers/specs/2026-09-19-photon-ui-reskin-design.md` — §5 (component language) is the authority; §2 for token names.

## Global Constraints

- Branch `feat/ui-reskin-grid`, off `main`. One PR. No release until PR 5 has merged.
- **The UI gate, from the repo root:** `npm run check` (0 errors **and** 0 warnings), `npm test`. No Rust changes; do not touch `crates/`.
- **Never launch the GUI** (`npm run dev`).
- Files in scope: `ui/src/components/Grid.svelte`, `Tile.svelte`, `Timeline.svelte`, `Toasts.svelte`, and `README.md`. Nothing else; not `ui/src/lib/**`.
- **Geometry is frozen.** A tile's outer box stays exactly `TILE`×`TILE` (160px, `box-sizing: border-box` is global); a header row stays exactly `HEADER` (32px) high; the timeline stays 44px wide. Do not change `layout.ts`, any `style:` binding, or any `bind:clientWidth/Height`.
- In those four components, after this PR: **no hex, `rgb(` or `rgba(` literal** in a `<style>` block, and **none of** `--bg --panel --panel-2 --muted`. Tokens only: `--surface --chrome --raised --field --hover --line --text --text-dim --accent --accent-soft --on-accent --danger --star`, scales `--r-1..4` (4/6/8/12px), `--s-1..6` (4/8/12/16/24/32px), `--t-1..5` (11/12/13/15/18px), `--shadow-menu`.
- No glyph icons remain in those components: `★ ⚠ ✕` become `<Icon>`.
- **No behaviour change:** every event handler, `bind:`, `aria-*`, `title`, `role`, `tabindex`, `{#if}`/`{#each}` structure, element order, text copy and existing comment stays exactly as it is.
- **Selection must not depend on focus.** Tiles are `tabindex="-1"`, and `tokens.css` removes the focus outline from every `[tabindex='-1']` element; the selected look comes from the `.selected` class alone.
- No logic changes, and there is no component test harness, so this PR adds no test; the commit messages say so.
- Comments carry reasoning, not mechanics.

---

### Task 1: The grid side, restyled

**Files:**
- Modify: `ui/src/components/Grid.svelte` (`<style>` only)
- Modify: `ui/src/components/Tile.svelte` (two glyphs; `<style>`)
- Modify: `ui/src/components/Timeline.svelte` (`<style>` only)
- Modify: `ui/src/components/Toasts.svelte` (one glyph; `<style>`)

**Interfaces:**
- Consumes: `Icon.svelte` — `import Icon from './Icon.svelte'`, props `{ name: IconName; size?: number; filled?: boolean }`; names used here: `star`, `triangle-alert`, `x`.

- [ ] **Step 1: Branch**

```bash
git switch main && git pull --ff-only && git switch -c feat/ui-reskin-grid
```

- [ ] **Step 2: `Grid.svelte` — replace the `<style>` block's contents with:**

```css
  .grid { display: flex; height: 100%; background: var(--surface); }
  .viewport { position: relative; flex: 1; min-width: 0; height: 100%; overflow-y: auto; outline: none; }
  .canvas { position: relative; }
  .header, .row { position: absolute; left: 0; right: 0; }
  /* 32px is layout.ts's HEADER: every row below is placed by it, so the type fits the box
     rather than the box growing to the type. */
  .header { display: flex; align-items: baseline; gap: var(--s-3); height: 32px; padding: 7px var(--s-2) 0; }
  .header .name { font-size: var(--t-4); font-weight: 600; white-space: nowrap; }
  .header .path { color: var(--text-dim); font-size: var(--t-1); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .row { display: flex; }
  .empty { position: absolute; inset: 0; display: grid; place-items: center; color: var(--text-dim); margin: 0; }
  .menu {
    position: fixed;
    z-index: 40;
    display: flex;
    flex-direction: column;
    min-width: 220px;
    max-height: 60vh;
    overflow-y: auto;
    padding: var(--s-1);
    background: var(--raised);
    border-radius: var(--r-3);
    /* The hairline is what separates a white menu from a white grid in light mode. */
    box-shadow: 0 0 0 1px var(--line), var(--shadow-menu);
  }
  .menu button { padding: 6px 10px; border: 0; border-radius: var(--r-2); background: none; text-align: left; cursor: pointer; }
  .menu button:hover { background: var(--hover); }
  .menu .album { padding-left: 18px; }
  .menu .heading {
    margin-top: var(--s-1);
    padding: 6px 10px 2px;
    color: var(--text-dim);
    font-size: var(--t-1);
    font-weight: 600;
    letter-spacing: 0.04em;
    border-top: 1px solid var(--line);
  }
  .menu .none { padding: 4px 18px 6px; color: var(--text-dim); font-size: var(--t-2); }
```

If the existing `.viewport` rule or any other rule carries a comment, keep the comment.

- [ ] **Step 3: `Tile.svelte`**

Import `Icon`. Two glyphs become icons; their wrappers keep class, `title` and `aria-label`:

```svelte
    <span class="broken" title="This photo can't be shown"><Icon name="triangle-alert" size={28} /></span>
```

```svelte
    <span class="star" aria-label="Starred"><Icon name="star" size={14} filled /></span>
```

Replace the `<style>` block's contents with:

```css
  .tile {
    position: relative;
    flex: none;
    padding: 0;
    border: 0;
    border-radius: var(--r-2);
    /* What shows while the thumbnail loads, and behind a broken one. */
    background: var(--field);
    overflow: hidden;
    cursor: default;
  }
  /* An outline, not a border: it sits outside the 160px box in the 8px gap between tiles,
     so the photo does not shrink when selected, and it reads over any photo in either
     theme. From the class and not from focus - tiles are tabindex="-1", and tokens.css
     takes the focus ring off those. */
  .tile.selected { outline: 2px solid var(--accent); outline-offset: 2px; }
  .tile.dimmed { opacity: 0.4; }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0;
    transition: opacity 120ms ease-out;
  }
  img.loaded { opacity: 1; }
  .broken { display: grid; place-items: center; height: 100%; color: var(--text-dim); }
  /* The shadow keeps an amber star legible on a bright or amber photo. */
  .star {
    position: absolute;
    right: 5px;
    bottom: 5px;
    color: var(--star);
    filter: drop-shadow(0 0 2px var(--shadow-ink));
    pointer-events: none;
  }
```

`--shadow-ink` does not exist yet: add it in Step 3b.

- [ ] **Step 3b: one new token, in `ui/src/tokens.css`**

This is the one permitted edit outside the four components. A drop shadow under a star must be dark in both themes (it sits on a photo, not on a surface), so it is a theme-independent token beside the shadows in the scales block (`:root { --r-1 … }`), directly after `--shadow-dialog`:

```css
  /* Ink for shadows cast onto photos, which are not themed: dark in both themes. */
  --shadow-ink: #000000b3;
```

`tokens.test.ts` must still pass unchanged (the scales block is not a theme block).

- [ ] **Step 4: `Timeline.svelte` — replace the `<style>` block's contents with:**

```css
  .timeline {
    position: relative;
    flex: none;
    width: 44px;
    height: 100%;
    overflow: visible;
    cursor: pointer;
    user-select: none;
    touch-action: none;
    border-left: 1px solid var(--line);
  }
  .year {
    position: absolute;
    left: 0;
    right: 0;
    color: var(--text-dim);
    font-size: var(--t-1);
    line-height: 14px;
    text-align: center;
    font-variant-numeric: tabular-nums;
    pointer-events: none;
  }
  .here {
    position: absolute;
    left: 4px;
    right: 4px;
    height: 2px;
    margin-top: -1px;
    background: var(--accent);
    border-radius: 1px;
    pointer-events: none;
  }
  .bubble {
    position: absolute;
    right: 100%;
    margin-right: 6px;
    transform: translateY(-50%);
    padding: 3px var(--s-2);
    background: var(--raised);
    border-radius: var(--r-2);
    box-shadow: 0 0 0 1px var(--line), var(--shadow-menu);
    font-size: var(--t-2);
    font-weight: 600;
    white-space: nowrap;
    pointer-events: none;
    z-index: 5;
  }
```

Keep `line-height: 14px` on `.year`: `timeline.ts` spaces the labels by it.

- [ ] **Step 5: `Toasts.svelte`**

Import `Icon`. The dismiss button's `✕` becomes `<Icon name="x" size={14} />`; its `onclick` and `aria-label` stay. Styles:

```css
  .toasts { position: fixed; right: var(--s-4); bottom: 40px; display: flex; flex-direction: column; gap: var(--s-2); z-index: 30; }
  .toast {
    display: flex;
    gap: var(--s-3);
    align-items: center;
    max-width: 420px;
    padding: 10px var(--s-3);
    background: var(--raised);
    border-left: 3px solid var(--danger);
    border-radius: var(--r-2);
    box-shadow: 0 0 0 1px var(--line), var(--shadow-menu);
  }
  .toast button {
    display: grid;
    place-items: center;
    flex: none;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: var(--r-2);
    background: none;
    color: var(--text-dim);
    cursor: pointer;
  }
  .toast button:hover { color: var(--text); background: var(--hover); }
```

- [ ] **Step 6: Check the constraints mechanically**

```bash
cd ui/src/components && for f in Grid Tile Timeline Toasts; do
  awk '/^<style>/{p=1} p' "$f.svelte" | grep -nE '#[0-9a-fA-F]{3,8}\b|rgba?\(|--(bg|panel|panel-2|muted)\b' && echo "LITERAL OR OLD NAME IN $f";
  grep -nP '[★☆⚠✕×]' "$f.svelte" && echo "GLYPH IN $f"; done; echo checked
```

Expected: only `checked`.

- [ ] **Step 7: Gate and commit**

```bash
npm run check && npm test
git add ui/src/components/Grid.svelte ui/src/components/Tile.svelte ui/src/components/Timeline.svelte ui/src/components/Toasts.svelte ui/src/tokens.css
git commit -m "feat(ui): the grid, tiles, timeline and toasts in the new component language

Tokens only, icons for the glyphs. A selected tile is an accent outline in the
gap between tiles rather than a border, so the photo no longer shrinks by 2px a
side when selected, and it comes from the class because tiles are
tabindex=-1. TILE, GAP and HEADER are untouched. No logic changes, so no test:
the look is on the README smoke checklist.
"
```

(End the message with your session's attribution trailer.)

---

### Task 2: README smoke checks

**Files:**
- Modify: `README.md` (`## Manual smoke checklist`)

- [ ] **Step 1: Add, after the shell items and in their style:**

```markdown
- [ ] In Light and in Dark: a selected tile has a blue outline that stays visible over a
      white, a black and a blue photo; selecting does not resize the photo; arrow-key
      selection shows the same outline.
- [ ] Scroll a long library top to bottom, then drag the timeline: folder headers never
      overlap the first tile row, and the year bubble follows the pointer.
- [ ] A starred tile's amber star is legible over a bright photo; a photo that cannot be
      shown has a warning icon, not an emoji.
- [ ] Right-click a tile in Light: the menu is distinct from the grid behind it. Provoke an
      error toast (open an offline folder's photo): it is readable in both themes.
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs(readme): smoke checks for the restyled grid"
```

(With your session's attribution trailer.)
