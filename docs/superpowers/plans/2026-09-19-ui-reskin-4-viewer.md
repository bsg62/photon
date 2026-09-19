# UI Reskin, PR 4 of 5: Viewer — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restyle the photo viewer's chrome — toolbar, crop bar, info panel, zoom control, close button, context menu, face boxes and crop overlay — as floating "glass" on an always-dark viewer, on tokens only, with icons in place of glyphs.

**Architecture:** CSS plus the minimum markup: icons inside the existing buttons, three separator spans in the toolbar, and one `data-theme="dark"` attribute on the viewer's root so its whole subtree resolves dark tokens in either app theme. No script change at all: `Viewer.svelte`'s `<script>` block is not touched. Geometry the script depends on (`.frame` at `100vw × 100vh`, the stage transform, the crop area and handle boxes, face box positions) is frozen.

**Tech Stack:** Svelte 5 runes, scoped component CSS, tokens and `Icon.svelte` from PR 1.

**Spec:** `docs/superpowers/specs/2026-09-19-photon-ui-reskin-design.md` — §5 "Viewer", §2 "Glass" and "Selectors".

## Global Constraints

- Branch `feat/ui-reskin-viewer`, off `main`. One PR. No release until PR 5 has merged.
- **The UI gate, from the repo root:** `npm run check` (0 errors **and** 0 warnings), `npm test`. No Rust changes; do not touch `crates/`.
- **Never launch the GUI** (`npm run dev`).
- Files in scope: `ui/src/components/Viewer.svelte`, `ui/src/tokens.css`, `README.md`. Nothing else; not `ui/src/lib/**`.
- **`Viewer.svelte`'s `<script>` block does not change except for one added `import Icon from './Icon.svelte';`.** Not a handler, effect, `$state`, `$derived` or constant. CLAUDE.md's "What reloads the viewer" section explains why this component's effect wiring is delicate; this PR stays out of it.
- **Frozen geometry** — these declarations stay byte-for-byte: `.viewer`'s `position/inset/z-index/display/place-items/overflow`; `.stage`; `.frame`; `img`; `.hidden`; `.outgoing` and `.outgoing.fading` (the 600ms is `CROSSFADE_MS`); `.crop-area`; every `.crop-handle` rule's position, size, margin and cursor; `.crop-rect`'s `position/box-sizing/cursor`; `.face`'s `position` and `pointer-events`; `.quiet` and the `.quiet .bar, .quiet .zoom, .quiet .close` rule; the `.bar, .zoom, .close` opacity transition. Only colours, radii, backgrounds, shadows, padding and type change around them.
- **Positions stay:** `.bar` bottom 12px centred; `.zoom` bottom 12px right 12px; `.close` top 12px right 12px; `.info` top 12px / right 56px / bottom 56px / width 280px.
- After this PR, `Viewer.svelte`'s `<style>` block has **no hex, `rgb(` or `rgba(` literal except the viewer's background `#000`**, and **none of** `--bg --panel --panel-2 --muted`. Tokens: `--surface --chrome --raised --field --hover --line --text --text-dim --accent --accent-soft --on-accent --danger --star --glass --glass-line`, scales `--r-1..4` (4/6/8/12px), `--s-1..6`, `--t-1..5` (11/12/13/15/18px), `--shadow-menu`, `--shadow-ink`, plus `--photo-line` and `--scrim`, the two new tokens of Task 1 Step 2.
- No glyph icons remain in `Viewer.svelte`: `★ ☆ ↺ ↻ ✂ ▶ ⏸ ⓘ ✕ ×` become `<Icon>`.
- **No behaviour change:** every event handler, `bind:`, `aria-*`, `title`, `role`, `tabindex`, `disabled`, `{#if}`/`{#each}`/`{#key}` structure, element order, text copy and existing comment stays exactly as it is. The only new elements are the three `<span class="sep" aria-hidden="true"></span>` separators.
- No logic changes and no component test harness, so no new test; the commit message says so. `tokens.test.ts` must pass unchanged.
- Comments carry reasoning, not mechanics.

---

### Task 1: The viewer, restyled

**Files:**
- Modify: `ui/src/tokens.css`
- Modify: `ui/src/components/Viewer.svelte`

**Interfaces:**
- Consumes: `Icon.svelte`, props `{ name: IconName; size?: number; filled?: boolean }`; names used: `star`, `rotate-ccw`, `rotate-cw`, `crop`, `play`, `pause`, `info`, `x`.

- [ ] **Step 1: Branch**

```bash
git switch main && git pull --ff-only && git switch -c feat/ui-reskin-viewer
```

- [ ] **Step 2: `ui/src/tokens.css`**

(a) `accent-color` is inherited, and declared once on `:root` it is computed there, in the app's theme; a dark subtree inside a light app would keep the light accent on its checkboxes and range input. Move it: delete `accent-color: var(--accent);` from the scales block (`:root { --r-1 … }`) and add it to the alias block (`:root, [data-theme] { --bg: … }`), with the block's comment extended by one sentence saying `accent-color` sits here for the same reason the aliases do.

(b) Two theme-independent tokens. In the scales block, beside `--shadow-ink` (added in PR 3, directly after `--shadow-dialog`; if `--shadow-ink` is missing, stop and report NEEDS_CONTEXT):

```css
  /* Drawn onto a photo - face boxes, the crop rectangle and its handles. The same in both
     themes, because the photo under it is. */
  --photo-line: #ffffffe6;
  /* Dims whatever is behind: the photo outside a crop, a face's name plate, and (from the
     reskin's last PR) the app behind a dialog. Dark in both themes. */
  --scrim: #000000a6;
```

- [ ] **Step 3: `Viewer.svelte` — the root**

Add `data-theme="dark"` to the root `<div class="viewer" …>`, after `class:quiet={slideshow.idle}`. Nothing else on that element changes. Put this comment directly above the attribute's line, inside the tag as Svelte allows, or immediately above the `<div` if the existing comment block sits there:

```svelte
<!-- Dark in both themes, so a photo is always judged against the same ground. tokens.css's
     theme blocks match any element, so this subtree resolves the dark tokens. -->
```

- [ ] **Step 4: `Viewer.svelte` — icons and separators**

Add `import Icon from './Icon.svelte';` to the script's imports. Then, leaving every attribute in place, replace only the glyph text inside these buttons:

| Today | Becomes |
|---|---|
| `{star.starred ? '★' : '☆'}` | `<Icon name="star" size={16} filled={star.starred} />` |
| `↺` | `<Icon name="rotate-ccw" size={16} />` |
| `↻` | `<Icon name="rotate-cw" size={16} />` |
| `✂` | `<Icon name="crop" size={16} />` |
| `{slideshow.active && slideshow.playing ? '⏸' : '▶'}` | `<Icon name={slideshow.active && slideshow.playing ? 'pause' : 'play'} size={16} />` |
| `ⓘ` | `<Icon name="info" size={16} />` |
| `✕` in `.close` | `<Icon name="x" size={16} />` |
| `×` in `.chip-remove` | `<Icon name="x" size={10} />` |

`grep -nP '[★☆↺↻✂▶⏸ⓘ✕×]' ui/src/components/Viewer.svelte` must come back empty afterwards, apart from occurrences inside comments or `title`/`aria-label` strings, which stay (a `×` in a dimension string such as `5472 × 3648`, if there is one, is text copy, not an icon: leave it and say so in your report).

In the normal toolbar (the `{:else}` branch's `<div class="bar">`), insert `<span class="sep" aria-hidden="true"></span>` at three places: after the star button; after the `{#if item?.edit}…{/if}` block (before the slideshow button); after the info button (before the caption button and its comment). The crop bar gets none.

- [ ] **Step 5: `Viewer.svelte` — styles**

Apply these replacements. A rule not listed here stays exactly as it is.

```css
  /* #000 is the one colour literal in the UI: the ground a photo is judged against is not
     a theme decision. `color` and `accent-color` are set here because both are inherited
     and were computed on :root, in the app's theme, before this subtree turned dark. */
  .viewer { position: fixed; inset: 0; z-index: 20; display: grid; place-items: center; background: #000; overflow: hidden; color: var(--text); accent-color: var(--accent); }
```

```css
  .face { position: absolute; border: 2px solid var(--photo-line); border-radius: var(--r-1); box-shadow: 0 0 0 1px var(--shadow-ink); pointer-events: none; }
  .face-name { position: absolute; left: -2px; top: 100%; margin-top: 2px; padding: 1px 6px; background: var(--scrim); border-radius: var(--r-1); color: var(--text); font-size: var(--t-2); white-space: nowrap; }
```

The glass, shared by every floating control (add this rule; then each control lists only what differs):

```css
  /* Glass: 85% opaque on its own, so it reads where backdrop-filter is slow or missing
     (some Linux GPUs); the blur is an enhancement on top. */
  .bar, .zoom, .close, .info {
    background: var(--glass);
    box-shadow: 0 0 0 1px var(--glass-line), var(--shadow-menu);
    -webkit-backdrop-filter: blur(18px);
    backdrop-filter: blur(18px);
  }
  .bar { position: absolute; bottom: 12px; left: 50%; transform: translateX(-50%); display: flex; align-items: center; gap: 2px; padding: var(--s-1); border-radius: var(--r-4); }
  .sep { width: 1px; height: 18px; margin: 0 var(--s-1); background: var(--glass-line); }
  .caption { padding: 0 10px; border: 0; background: none; color: var(--text-dim); font-size: var(--t-2); white-space: nowrap; cursor: pointer; }
  .caption:hover:not(:disabled) { color: var(--text); }
  .caption:disabled { cursor: default; }
```

```css
  .menu {
    position: fixed;
    z-index: 40;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    padding: var(--s-1);
    background: var(--raised);
    border-radius: var(--r-3);
    box-shadow: 0 0 0 1px var(--line), var(--shadow-menu);
  }
  .menu button { padding: 6px 10px; border: 0; border-radius: var(--r-2); background: none; text-align: left; cursor: pointer; }
  .menu button:hover { background: var(--hover); }
  .zoom { position: absolute; bottom: 12px; right: 12px; display: flex; align-items: center; gap: var(--s-2); padding: 6px var(--s-3); border-radius: var(--r-4); }
  .zoom input { width: 120px; }
  .level { color: var(--text-dim); font-size: var(--t-2); min-width: 38px; text-align: right; font-variant-numeric: tabular-nums; }
  .close { position: absolute; top: 12px; right: 12px; display: grid; place-items: center; width: 32px; height: 32px; padding: 0; border: 0; border-radius: 50%; color: var(--text-dim); cursor: pointer; }
  .close:hover { color: var(--text); }
  .star, .tool { display: grid; place-items: center; width: 30px; height: 30px; padding: 0; border: 0; border-radius: var(--r-3); background: none; color: var(--text-dim); font-size: var(--t-2); line-height: 1; cursor: pointer; transition: background-color 120ms ease-out; }
  .star:hover:not(:disabled), .tool:hover:not(:disabled) { color: var(--text); background: var(--hover); }
  .star[aria-pressed='true'] { color: var(--star); }
  .tool[aria-pressed='true'] { background: var(--accent); color: var(--on-accent); }
  .star:disabled, .tool:disabled { cursor: default; opacity: 0.4; }
  .tool.wide { width: auto; padding: 0 var(--s-3); color: var(--text); }
  .tool.primary, .tool.primary:hover:not(:disabled) { background: var(--accent); color: var(--on-accent); }
  .aspect { height: 30px; padding: 0 var(--s-2); border: 0; border-radius: var(--r-3); background: var(--field); color: var(--text); font: inherit; font-size: var(--t-2); }
  @media (prefers-reduced-motion: reduce) { .star, .tool { transition: none; } }
```

Check before replacing `.star:disabled, .tool:disabled`: today a disabled tool only loses its pointer cursor, and its colour is already the dim one. `opacity: 0.4` is new and deliberate: with every tool now the same dim grey at rest, a disabled one needs to look different from an enabled one.

```css
  .crop-rect { position: absolute; box-sizing: border-box; border: 1px solid var(--photo-line); box-shadow: 0 0 0 9999px var(--scrim); cursor: move; }
  .crop-handle::after { content: ''; position: absolute; inset: 7px; background: var(--photo-line); border-radius: 1px; box-shadow: 0 0 0 1px var(--shadow-ink); }
  .error { color: var(--text-dim); }
  .info {
    position: absolute;
    top: 12px;
    right: 56px;
    bottom: 56px;
    width: 280px;
    overflow: auto;
    padding: 14px var(--s-4);
    border-radius: var(--r-4);
    color: var(--text);
    font-size: var(--t-3);
    user-select: text;
  }
  .info-title { margin: 0 0 2px; font-size: var(--t-4); font-weight: 600; overflow-wrap: anywhere; }
  .info-path { margin: 0 0 10px; color: var(--text-dim); font-size: var(--t-1); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .info h3 { margin: var(--s-3) 0 var(--s-1); color: var(--text-dim); font-size: var(--t-1); font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; }
  .info dt { color: var(--text-dim); }
  .info-link { padding: 0; border: 0; background: none; color: var(--accent); font: inherit; text-align: left; cursor: pointer; overflow-wrap: anywhere; }
  .info-muted { margin: 0; color: var(--text-dim); }
  .chips li { display: inline-flex; align-items: center; padding: 2px var(--s-2); background: var(--field); border-radius: 999px; font-size: var(--t-2); }
  .copies { margin: 0; padding: 0; list-style: none; font-size: var(--t-2); }
  .chip-remove {
    display: grid;
    place-items: center;
    margin-left: var(--s-1);
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    cursor: pointer;
    opacity: 0.6;
  }
  .tag-input { width: 100%; margin-top: 6px; box-sizing: border-box; padding: 5px var(--s-2); border: 0; border-radius: var(--r-2); background: var(--field); color: var(--text); font: inherit; }
  .tag-input::placeholder { color: var(--text-dim); }
```

The rules `.info dl`, `.info dd`, `.info-link:hover`, `.chips`, `.albums`, `.copies li`, `.albums label`, `.chip-remove:hover:not(:disabled)`, `.chip-remove:disabled` stay as they are.

- [ ] **Step 6: Check the constraints mechanically**

```bash
cd ui/src/components && awk '/^<style>/{p=1} p' Viewer.svelte | grep -nE '#[0-9a-fA-F]{3,8}\b|rgba?\(|--(bg|panel|panel-2|muted)\b'
awk '/^<script/{p=1} /^<\/script>/{print; p=0} p' Viewer.svelte > /tmp/viewer-script.after
git show main:ui/src/components/Viewer.svelte | awk '/^<script/{p=1} /^<\/script>/{print; p=0} p' | diff - /tmp/viewer-script.after
```

Expected: the first prints exactly one line, the `.viewer` rule with `#000`. The `diff` prints exactly one added line, the `Icon` import. Paste both outputs in your report, then `rm /tmp/viewer-script.after`.

- [ ] **Step 7: Gate and commit**

```bash
npm run check && npm test
git add ui/src/components/Viewer.svelte ui/src/tokens.css
git commit -m "feat(viewer): floating glass chrome on an always-dark viewer

The viewer's root carries data-theme=dark, so its subtree resolves the dark
tokens in either app theme and a photo is always judged against the same
ground. One toolbar groups the tools with separators; the info panel, zoom and
close share its glass, which is 85% opaque by itself so it reads without
backdrop-filter. Face boxes and the crop overlay keep their geometry and take
photo-overlay tokens. accent-color moves beside the aliases in tokens.css, or
the viewer's checkboxes would keep the light accent in a light app.

The script block is unchanged but for the Icon import: no handler, effect or
constant moved, so nothing that feeds pictureChanged is touched. No logic
changes, so no test; the look is on the README smoke checklist.
"
```

(End the message with your session's attribution trailer.)

---

### Task 2: README smoke checks

**Files:**
- Modify: `README.md` (`## Manual smoke checklist`)

- [ ] **Step 1: Add, after the grid items and in their style:**

```markdown
- [ ] With photon in Light, open a photo: the viewer is black and its toolbar, info panel,
      zoom control, menu and checkboxes are all dark, with the dark theme's blue.
- [ ] The toolbar reads over a white photo and over a black one; on a machine where the
      blur is missing it is still legible. Star, rotate, crop, slideshow and info all still
      work, and a disabled tool looks disabled.
- [ ] Crop: the rectangle, its eight handles and the dimming outside it are where they were;
      dragging a handle, Apply, Cancel and Whole photo behave as before.
- [ ] Info panel open, star a photo, add and remove a keyword, tick an album: the photo on
      screen does not reload, and zoom and pan are kept.
- [ ] Slideshow: after a few seconds without the pointer the toolbar, zoom and close fade
      out, and come back when it moves.
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs(readme): smoke checks for the restyled viewer"
```

(With your session's attribution trailer.)
