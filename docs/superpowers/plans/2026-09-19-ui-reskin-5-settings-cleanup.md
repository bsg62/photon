# UI Reskin, PR 5 of 5: Settings and Cleanup — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restyle the Settings dialog, delete the old variable aliases, and add the tripwire test that defines "the reskin is finished": no colour literal, no old variable name and no glyph icon in any component.

**Architecture:** Task 1 is CSS and one glyph in `Settings.svelte`, plus one new token. Task 2 removes the alias block from `tokens.css` and adds `no-literals.test.ts`, which reads every `.svelte` file as raw text through `import.meta.glob` (verified to work under this vitest setup). Task 3 is documentation.

**Tech Stack:** Svelte 5 runes, scoped CSS, vitest (node environment, `test.css: true`).

**Spec:** `docs/superpowers/specs/2026-09-19-photon-ui-reskin-design.md` — §5 "Settings dialog", §6 item 5, §7 `no-literals.test.ts`.

## Global Constraints

- Branch `feat/ui-reskin-settings`, off `main`. One PR.
- **The UI gate, from the repo root:** `npm run check` (0 errors **and** 0 warnings), `npm test`. No Rust changes; do not touch `crates/`.
- **Never launch the GUI** (`npm run dev`).
- **No behaviour change in `Settings.svelte`:** its `<script>` block changes only by one added `import Icon from './Icon.svelte';`. Every event handler, `bind:`, `aria-*`, `title`, `role`, `tabindex`, `disabled`, `{#if}`/`{#each}` structure, element order, text copy and existing comment stays. The `.backdrop` rule's `position/inset/z-index/display/grid-template/place-items/padding` and its comment stay byte-for-byte (the comment records a real layout bug), as do `.dialog`'s `display/flex-direction/width/height/outline`.
- Tokens available: `--surface --chrome --raised --field --hover --line --text --text-dim --accent --accent-soft --on-accent --danger --star --glass --glass-line`, scales `--r-1..4` (4/6/8/12px), `--s-1..6` (4/8/12/16/24/32px), `--t-1..5` (11/12/13/15/18px), `--shadow-menu`, `--shadow-dialog`, `--shadow-ink`, `--photo-line`, `--scrim`, and `--field-hover` from Task 1.
- **Every new test is demonstrated to fail with its change reverted**, using the probes written in the task. A compile error is not proof.
- Comments carry reasoning, not mechanics.

---

### Task 1: The Settings dialog, restyled

**Files:**
- Modify: `ui/src/tokens.css` (one token, both theme blocks)
- Modify: `ui/src/components/Settings.svelte`
- Modify: `ui/src/components/FolderTree.svelte` (one declaration)
- Modify: `ui/src/components/Viewer.svelte` (one rule in the style block)

- [ ] **Step 1: Branch**

```bash
git switch main && git pull --ff-only && git switch -c feat/ui-reskin-settings
```

- [ ] **Step 2: `--field-hover`**

A ghost button rests on `--field`; `--hover` is *fainter* than `--field`, so hovering one would lighten it. In `tokens.css`, directly after `--field` in each theme block:

- light: `--field-hover: #00000018;`
- dark: `--field-hover: #ffffff1f;`

with this comment above the light one only: `/* A ghost button under the pointer. --hover is for things that rest on nothing; it is fainter than --field, so it cannot serve here. */`

In `FolderTree.svelte`, the rule `.add:hover { background: var(--hover); }` becomes `.add:hover { background: var(--field-hover); }`.

- [ ] **Step 2b: the viewer's caption at narrow widths**

Found by PR 4's review: at the 800px minimum window width the toolbar's right end runs under
the zoom control, and since both are now opaque glass the end of the caption is covered. In
`Viewer.svelte`'s style block only, the `.caption` rule gains three declarations, with this
comment above it:

```css
  /* The file name is the only part of the toolbar that can be any length; at the minimum
     window width an unbounded one ran the toolbar under the zoom control. */
  .caption { …existing declarations…; max-width: 34vw; overflow: hidden; text-overflow: ellipsis; }
```

`white-space: nowrap` is already there. One comment fix in the same block: the comment above
the `[aria-pressed='true']` rules says `.tool.primary` is "above"; it is below — make the
sentence true. Nothing else in `Viewer.svelte` changes; its script
block must be byte-identical to `main`'s (check with the same `awk … | diff` as PR 4's Step 6
and paste the empty diff in your report).

- [ ] **Step 3: `Settings.svelte` — the close glyph**

Add `import Icon from './Icon.svelte';`. In the header's close button, `✕` becomes `<Icon name="x" size={16} />`; its `class`, `aria-label` and `onclick` stay.

- [ ] **Step 4: `Settings.svelte` — styles**

Apply these replacements. A rule not listed stays as it is (`.body`, `.meta`, `.name`, `.offline .name`, `.actions`, `.library`, `.selectable`, `.interval`, `.folders`, `.tags`, `dd`, the `.status.*` pair if it already uses tokens).

```css
  /* (keep the existing comment and every other declaration of .backdrop) */
    background: var(--scrim);
```

```css
  .dialog {
    display: flex;
    flex-direction: column;
    width: min(720px, 100%);
    height: min(520px, 100%);
    /* Clips the header's and the section list's chrome to the rounded corners. */
    overflow: hidden;
    background: var(--surface);
    border-radius: var(--r-4);
    box-shadow: 0 0 0 1px var(--line), var(--shadow-dialog);
    outline: none;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px var(--s-3) 10px var(--s-4);
    background: var(--chrome);
    border-bottom: 1px solid var(--line);
  }
  h1 { margin: 0; font-size: var(--t-4); font-weight: 600; }
  h2 { margin: 0 0 var(--s-1); font-size: var(--t-4); font-weight: 600; }
  button {
    padding: 5px var(--s-3);
    border: 0;
    border-radius: var(--r-3);
    background: var(--field);
    cursor: pointer;
    transition: background-color 120ms ease-out;
  }
  button:hover:not(:disabled) { background: var(--field-hover); }
  button:disabled { color: var(--text-dim); opacity: 0.6; cursor: default; }
  .close { display: grid; place-items: center; width: 28px; height: 28px; padding: 0; background: none; color: var(--text-dim); }
  .close:hover:not(:disabled) { background: var(--hover); color: var(--text); }
  .add { background: var(--accent); color: var(--on-accent); font-weight: 600; }
  /* Spelled to out-rank the generic hover above, which would otherwise grey it. */
  .add:hover:not(:disabled) { background: var(--accent); filter: brightness(1.08); }
  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 150px;
    padding: var(--s-2) 6px;
    background: var(--chrome);
    border-right: 1px solid var(--line);
  }
  nav button { height: 28px; padding: 0 var(--s-2); background: none; text-align: left; }
  nav button:hover:not(:disabled) { background: var(--hover); }
  nav button.active, nav button.active:hover:not(:disabled) { background: var(--accent-soft); }
  section { flex: 1; min-width: 0; padding: var(--s-3) var(--s-4); overflow: auto; }
  .hint, .empty { margin: 0 0 var(--s-3); color: var(--text-dim); }
  .interval input { width: 64px; padding: 5px var(--s-2); border: 0; border-radius: var(--r-2); background: var(--field); color: inherit; font: inherit; }
  .segmented { display: inline-flex; gap: 2px; padding: 2px; border-radius: var(--r-3); background: var(--field); }
  .segmented button { padding: 4px 14px; border-radius: var(--r-2); background: none; }
  .segmented button:hover:not(:disabled) { background: var(--hover); }
  /* After the hover rule and spelled as long, so the chosen segment keeps its accent under
     the pointer by specificity rather than by source order alone. */
  .segmented button.checked, .segmented button.checked:hover:not(:disabled) { background: var(--accent); color: var(--on-accent); }
  .folders li, .tags li { border-bottom: 1px solid var(--line); }
  .path { overflow: hidden; color: var(--text-dim); font-size: var(--t-2); text-overflow: ellipsis; white-space: nowrap; }
  .details { color: var(--text-dim); font-size: var(--t-2); }
  .danger { color: var(--danger); }
  dt { color: var(--text-dim); }
  .filter, .rename {
    width: 100%;
    padding: 5px var(--s-2);
    border: 0;
    border-radius: var(--r-2);
    background: var(--field);
    color: inherit;
    font: inherit;
  }
  .filter::placeholder { color: var(--text-dim); }
  .error { color: var(--danger); font-size: var(--t-2); }
  @media (prefers-reduced-motion: reduce) { button { transition: none; } }
```

For `.folders li` and `.tags li`: keep their existing `display/align-items/gap/padding` declarations and change only the `border-bottom` colour to `var(--line)`; the combined rule above is shorthand for that, not a replacement of their layout.

If `.add` is not the class of the "Add folder…" button in this file, find that button and report what you found rather than guessing.

- [ ] **Step 5: Check, gate, commit**

```bash
awk '/^<style>/{p=1} p' ui/src/components/Settings.svelte | grep -nE '#[0-9a-fA-F]{3,8}\b|rgba?\(|--(bg|panel|panel-2|muted)\b'; grep -nP '[✕×]' ui/src/components/Settings.svelte; echo checked
npm run check && npm test
git add ui/src/tokens.css ui/src/components/Settings.svelte ui/src/components/FolderTree.svelte ui/src/components/Viewer.svelte
git commit -m "feat(settings): the dialog in the new component language

Content on the surface, the header and the section list on chrome with the
sidebar's rows, so the dialog reads as the app in miniature. Ghost buttons
get --field-hover: --hover is fainter than --field, so it lightened a button
under the pointer. The segmented control's chosen segment now keeps its accent
on hover by specificity, not source order. The viewer's caption is capped and
ellipsised, so the toolbar no longer runs under the zoom control at the minimum
window width. No logic changes, so no test.
"
```

Expected from the first line: only `checked`. (End the message with your session's attribution trailer.)

---

### Task 2: Delete the aliases; the tripwire test

**Files:**
- Modify: `ui/src/tokens.css`
- Modify: `ui/src/lib/tokens.test.ts`
- Create: `ui/src/lib/no-literals.test.ts`

- [ ] **Step 1: Confirm nothing uses an old name**

```bash
grep -rnE 'var\(--(bg|panel|panel-2|muted)\)' ui/src ui/index.html
```

Expected: only the four declarations inside `tokens.css`'s alias block. If any component still uses one, stop and report it (NEEDS_CONTEXT): a dangling `var(--bg)` computes to nothing, silently.

- [ ] **Step 2: Write the failing test — `ui/src/lib/no-literals.test.ts`**

```ts
import { describe, expect, it } from 'vitest';
import appCss from '../app.css?raw';

/** Every component, as text. The reskin's definition of done, and what keeps it done: a
 *  colour belongs in tokens.css, an icon in icons.ts. */
const components = import.meta.glob('../**/*.svelte', { query: '?raw', import: 'default', eager: true }) as Record<
  string,
  string
>;

/** The viewer's ground. Not a theme decision: a photo is judged against black. */
const ALLOWED = [{ file: '../components/Viewer.svelte', literal: '#000' }];

/** Removed with the aliases; an unknown custom property computes to nothing, silently. */
const OLD_NAMES = ['--bg', '--panel', '--panel-2', '--muted'];

const GLYPHS = /[⚙★☆🕘⧉▸▾⚠✕↺↻✂▶⏸ⓘ]/u;

function styleOf(source: string): string {
  return (/<style[^>]*>([\s\S]*?)<\/style>/.exec(source)?.[1] ?? '').replace(/\/\*[\s\S]*?\*\//g, '');
}

/** Markup and script without comments, where a glyph may be mentioned in prose. */
function codeOf(source: string): string {
  return source
    .replace(/<style[^>]*>[\s\S]*?<\/style>/, '')
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/^\s*\/\/.*$/gm, '');
}

function literals(css: string): string[] {
  return css.match(/#[0-9a-fA-F]{3,8}\b|rgba?\([^)]*\)|hsla?\([^)]*\)/g) ?? [];
}

const styled = [...Object.entries(components), ['../app.css', appCss] as const];

describe('component styles', () => {
  it('finds the components', () => {
    expect(Object.keys(components).length).toBeGreaterThan(8);
    expect(styleOf(components['../components/Tile.svelte'])).toContain('.tile');
  });

  it.each(styled)('%s has no colour literal', (file, source) => {
    const css = file.endsWith('.css') ? source.replace(/\/\*[\s\S]*?\*\//g, '') : styleOf(source);
    const found = literals(css).filter((l) => !ALLOWED.some((a) => a.file === file && a.literal === l));
    expect(found).toEqual([]);
  });

  it.each(styled)('%s uses none of the removed variable names', (file, source) => {
    const used = OLD_NAMES.filter((name) => new RegExp(`var\\(\\s*${name}\\s*[,)]`).test(source));
    expect(used).toEqual([]);
  });

  it.each(Object.entries(components))('%s draws no icon with a glyph', (_file, source) => {
    expect(GLYPHS.exec(codeOf(source))?.[0]).toBeUndefined();
  });
});
```

- [ ] **Step 3: Run it**

Run: `npm test -w ui -- src/lib/no-literals.test.ts`
Expected: everything passes already (PRs 2–5 did the work), **except** nothing fails for the old names because the alias block lives in `tokens.css`, which this test does not read. That is correct: the probes in Step 6 are what prove the test. If a component fails, fix the component only if the fix is a one-token substitution; otherwise stop and report.

`×` is deliberately not in `GLYPHS`: it is legitimate text in a dimension string (`5472 × 3648`).

- [ ] **Step 4: Delete the aliases — `ui/src/tokens.css`**

Remove the four alias declarations and the part of the block comment about the old names. Keep the block itself: do not fold `accent-color` back into the scales `:root` block (a test added in PR 4 fails if you do). `accent-color: var(--accent);` stays in that `:root, [data-theme]` block, with a comment that still explains why it is declared on `[data-theme]` and not on `:root` alone (it is inherited and would otherwise be computed once, in the app's theme, leaving the always-dark viewer with the light accent).

- [ ] **Step 5: `ui/src/lib/tokens.test.ts`**

The test `the old names are aliased on [data-theme] as well as :root` (in `describe('the structure of tokens.css')`) now fails. Replace it with:

```ts
  it('no longer defines the names the components used before the tokens', () => {
    for (const name of ['--bg', '--panel', '--panel-2', '--muted']) {
      expect(css).not.toMatch(new RegExp(`${name}\\s*:`));
    }
  });
```

(Adapt the test's exact old title to what the file has; keep the block-order test untouched.)

- [ ] **Step 6: Prove the tests discriminate**

Apply each probe, run `npm test -w ui -- src/lib/no-literals.test.ts src/lib/tokens.test.ts`, see the named test fail, undo exactly; `git diff` must be clean of probe residue at the end.

1. In `Tile.svelte`'s `<style>`, change `color: var(--star);` to `color: #ffcf40;` → "has no colour literal" fails for `Tile.svelte` only.
2. In `StatusBar.svelte`'s `<style>`, change one `var(--text-dim)` to `var(--muted)` → "uses none of the removed variable names" fails for `StatusBar.svelte`.
3. In `Toasts.svelte`'s markup, replace the `<Icon name="x" … />` with `✕` → "draws no icon with a glyph" fails for `Toasts.svelte`.
4. In `Viewer.svelte`'s `<style>`, change `.viewer`'s `#000` to `#111` → "has no colour literal" fails for `Viewer.svelte` (the allow-list is exact).
5. In `tokens.css`, add `--muted: var(--text-dim);` back to the `:root, [data-theme]` block → the `tokens.test.ts` test from Step 5 fails.

- [ ] **Step 7: Gate and commit**

```bash
npm run check && npm test
git add ui/src/tokens.css ui/src/lib/tokens.test.ts ui/src/lib/no-literals.test.ts
git commit -m "test(ui): no colour literal, old variable name or glyph icon in any component

The old names' aliases are gone from tokens.css, and no-literals.test.ts reads
every component as text to keep it that way: a colour belongs in tokens.css,
an icon in icons.ts, and a dangling var(--bg) would compute to nothing
silently. The viewer's #000 is the one allowed literal. Five probes, one per
assertion, each failed its test.
"
```

(With your session's attribution trailer.)

---

### Task 3: Documentation

**Files:**
- Modify: `README.md` (`## Manual smoke checklist`)
- Modify: `CLAUDE.md`
- Modify: `docs/superpowers/specs/2026-09-19-photon-ui-reskin-design.md` (status line)

- [ ] **Step 1: README — add after the viewer items, in their style:**

```markdown
- [ ] Settings in Light and in Dark: the header and section list are tinted, the content is
      not; the active section is a blue-tinted row; every button darkens slightly under the
      pointer, "Add folder…" is the one blue button, and "Remove…" is red text.
- [ ] Settings → Tags with hundreds of tags: the dialog stays centred and its corners stay
      rounded while the list scrolls.
```

Also reword the earlier item that says the whole window "follows at once" only if its wording is now inaccurate; if it reads true with every surface restyled, leave it.

- [ ] **Step 2: CLAUDE.md**

Read the file's voice first: dense paragraphs, each explaining a trap and its why. Add a section `### Styling` at the end of `## Architecture` (before `## Conventions`), of two or three short paragraphs covering exactly these facts, in that voice:

- Every colour is a token in `ui/src/tokens.css`, in a light and a dark block selected by `data-theme` on `<html>`; `no-literals.test.ts` fails on a colour literal, a removed variable name or a glyph icon in any component, and `tokens.test.ts` holds the palette to WCAG contrast, light/dark parity and the dark-after-light block order (equal specificity on `<html>`, so source order is what lets dark win). Icons are `Icon.svelte` over vendored Lucide path data in `lib/icons.ts`; a new icon is copied from `lucide-static` and its licence is already in `THIRD-PARTY-NOTICES.md`.
- The theme blocks match any element, which is how the viewer is dark in both themes (`data-theme="dark"` on its root). An inherited property set from a token on `:root` (`color`, `accent-color`) is computed there, in the app's theme, so a themed subtree must set it again on its own root.
- The theme choice lives in the `settings` table; `theme-boot.js` applies a `localStorage` mirror before first paint because the database answers too late, and it is a file rather than an inline script because the CSP forbids inline scripts. The database wins when the two disagree. `createTheme` is generation-counted like `LibraryStore`, because the singleton outlives an App remount.
- `[tabindex='-1']:focus-visible { outline: none }` is global, for script-focused containers. A roving-tabindex widget's items carry `tabindex="-1"` too and would silently lose their focus ring: scope the rule before adding one. For the same reason a tile's selection outline comes from `.selected`, not from focus.
- The look cannot be tested here, but it can be seen without launching the app: build the UI, serve `ui/dist` with a script that fakes `window.__TAURI_INTERNALS__.invoke` with canned data, and screenshot it in headless Chromium (`--screenshot`, with `--force-dark-mode` or not); thumbnails can be served by mapping `photon.localhost` with `--host-resolver-rules` and a Windows user agent, since `mediaUrl` uses `http://photon.localhost` there.

Also, in the `## Commands` paragraph that lists what layout can be measured without the GUI, nothing changes.

- [ ] **Step 3: Spec status**

In the spec's header, `**Status:** Approved design, not yet implemented` becomes `**Status:** Approved design, implemented (PRs #46–#NN)` — leave `#NN` literally as written; the controller fills in the number when the PR exists.

- [ ] **Step 4: Commit**

```bash
git add README.md CLAUDE.md docs/superpowers/specs/2026-09-19-photon-ui-reskin-design.md
git commit -m "docs: styling in CLAUDE.md, Settings smoke checks, the reskin spec marked implemented"
```

(With your session's attribution trailer.)
