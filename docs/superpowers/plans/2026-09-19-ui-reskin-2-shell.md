# UI Reskin, PR 2 of 5: Shell — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restyle the app's shell — top bar, search field, sidebar, splitter and status bar — in the new component language, on tokens only, with icons in place of glyphs.

**Architecture:** CSS and markup only, in four components plus the root font size. No logic, handler, DOM-order or layout-grid change. Each component's `<style>` block ends the PR with no colour literal and none of the eight old variable names.

**Tech Stack:** Svelte 5 runes, scoped component CSS, the tokens and `Icon.svelte` from PR 1.

**Spec:** `docs/superpowers/specs/2026-09-19-photon-ui-reskin-design.md` — §5 (component language) is the authority; §2 for token names.

## Global Constraints

- Branch `feat/ui-reskin-shell`, off `main`. One PR. No release until PR 5 has merged.
- **The UI gate, from the repo root:** `npm run check` (0 errors **and** 0 warnings), `npm test`. No Rust changes, so no Rust gate; do not touch `crates/`.
- **Never launch the GUI** (`npm run dev`). What needs eyes goes on the README's smoke checklist.
- Files in scope: `ui/src/app.css`, `ui/src/App.svelte`, `ui/src/components/SearchBar.svelte`, `ui/src/components/FolderTree.svelte`, `ui/src/components/StatusBar.svelte`, `README.md`. Nothing else.
- In those four components, after this PR: **no hex, `rgb(` or `rgba(` literal** in a `<style>` block, and **none of** `--bg --panel --panel-2 --muted`. Tokens only: `--surface --chrome --raised --field --hover --line --text --text-dim --accent --accent-soft --on-accent --danger --star`, scales `--r-1..4` (4/6/8/12px), `--s-1..6` (4/8/12/16/24/32px), `--t-1..5` (11/12/13/15/18px), `--shadow-menu`.
- No glyph icons remain in those components: `⚙ ★ 🕘 ⧉ ▸ ▾` become `<Icon>`.
- **No behaviour change:** every event handler, `bind:`, `aria-*`, `title`, `role`, `tabindex`, `inert`, `{#if}`/`{#each}` structure and element order stays exactly as it is. Text copy stays, with one spec-mandated exception: counts lose their parentheses (`(13)` → `13`).
- Do not change `ui/src/lib/**` or the `.app` grid template in `App.svelte` (`grid-template-columns: var(--sidebar-width) 5px 1fr`).
- There is no component test harness (vitest is `environment: 'node'`), and this PR changes no logic, so it adds no test; the commit messages say so.
- Comments carry reasoning, not mechanics. Keep every existing comment unless the thing it explains is gone.

---

### Task 1: The shell, restyled

**Files:**
- Modify: `ui/src/app.css`
- Modify: `ui/src/App.svelte` (gear button markup; `<style>`)
- Modify: `ui/src/components/SearchBar.svelte` (field wrapper markup; `<style>`)
- Modify: `ui/src/components/FolderTree.svelte` (icons and counts in markup; `<style>`)
- Modify: `ui/src/components/StatusBar.svelte` (`<style>`)

**Interfaces:**
- Consumes: `Icon.svelte` — `import Icon from './Icon.svelte'` (from `App.svelte`: `./components/Icon.svelte`), props `{ name: IconName; size?: number; filled?: boolean }`; names used here: `settings`, `search`, `star`, `clock`, `copy`, `chevron-right`, `chevron-down`, `folder`, `user`, `tag`.

- [ ] **Step 1: Branch**

```bash
git switch main && git pull --ff-only && git switch -c feat/ui-reskin-shell
```

- [ ] **Step 2: Root font size — `ui/src/app.css`**

Change `font-size: 14px;` to `font-size: var(--t-3);`. Nothing else in the file.

- [ ] **Step 3: `App.svelte`**

Import: `import Icon from './components/Icon.svelte';` beside the other component imports.

The gear button's content `⚙` becomes an icon; every attribute stays:

```svelte
    <button class="gear" bind:this={gear} aria-label="Settings" title="Settings" onclick={() => openSettings('folders')}
      ><Icon name="settings" size={18} /></button
    >
```

Replace the `<style>` block's rules for `.sidebar`, `.splitter`, `.topbar`, `.gear` (keep `.app`, `.content`, `.statusbar` and all comments as they are):

```css
  .sidebar {
    overflow: auto;
    background: var(--chrome);
  }
  .splitter {
    cursor: col-resize;
    touch-action: none;
    background: var(--chrome);
    border-left: 1px solid var(--line);
  }
  /* Its own focus treatment rather than the global ring: a 5px bar cannot hold one. */
  .splitter:hover,
  .splitter:focus-visible {
    background: var(--accent);
    outline: none;
  }
  .topbar {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-right: var(--s-2);
    background: var(--chrome);
    border-bottom: 1px solid var(--line);
  }
  .gear {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 0;
    border-radius: var(--r-3);
    background: none;
    color: var(--text-dim);
    cursor: pointer;
    transition: background-color 120ms ease-out;
  }
  .gear:hover { color: var(--text); background: var(--hover); }
  @media (prefers-reduced-motion: reduce) { .gear { transition: none; } }
```

`.gear:focus-visible` is deleted on purpose: the global ring from `tokens.css` is the focus treatment now. Keep the comment above `.topbar` ("Its own grid row, so it stays put…").

- [ ] **Step 4: `SearchBar.svelte`**

Import `Icon` (`import Icon from './Icon.svelte';`). Wrap nothing new around the input beyond one positioned label-less span for the icon; the `<input>` keeps every attribute and handler and its comment:

```svelte
<div class="bar">
  <div class="field">
    <Icon name="search" size={14} />
    <input … unchanged … />
  </div>
</div>
```

Styles (replace `.search`; keep `.bar` and its comment):

```css
  .bar { display: flex; flex: 1; min-width: 0; padding: var(--s-2); }
  .field {
    position: relative;
    display: flex;
    align-items: center;
    width: 320px;
    max-width: 100%;
    color: var(--text-dim);
  }
  /* The icon sits over the input's left padding; clicks pass through to the input. */
  .field :global(svg) { position: absolute; left: 9px; pointer-events: none; }
  .search {
    width: 100%;
    box-sizing: border-box;
    padding: 6px var(--s-2) 6px 30px;
    border: 0;
    border-radius: var(--r-3);
    background: var(--field);
    color: var(--text);
    font: inherit;
  }
  .search::placeholder { color: var(--text-dim); }
  /* The global ring, pulled in to hug the field rather than float 2px off it. */
  .search:focus-visible { outline-offset: 0; }
```

- [ ] **Step 5: `StatusBar.svelte`**

Markup unchanged. Styles:

```css
  .status {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--s-3);
    padding: var(--s-1) var(--s-3);
    background: var(--chrome);
    color: var(--text-dim);
    font-size: var(--t-2);
    border-top: 1px solid var(--line);
  }
  .notices { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 16px; min-width: 0; }
  .scan { display: inline-flex; align-items: center; gap: var(--s-2); white-space: nowrap; }
  progress {
    width: 120px;
    height: 6px;
    appearance: none;
    border: 0;
    border-radius: 3px;
    background: var(--line);
    overflow: hidden;
  }
  progress::-webkit-progress-bar { background: var(--line); border-radius: 3px; }
  progress::-webkit-progress-value { background: var(--accent); border-radius: 3px; transition: width 200ms ease-out; }
  progress::-moz-progress-bar { background: var(--accent); border-radius: 3px; }
```

- [ ] **Step 6: `FolderTree.svelte` — markup**

Import `Icon`. Change only these things, leaving every attribute, handler, comment and block structure alone:

1. The three root rows. The glyph leaves the text and becomes an icon before it:
   - `<span class="name">★ Starred</span>` → `<Icon name="star" size={14} /><span class="name">Starred</span>`
   - `<span class="name">🕘 Recent</span>` → `<Icon name="clock" size={14} /><span class="name">Recent</span>`
   - `<span class="name">⧉ Duplicates</span>` → `<Icon name="copy" size={14} /><span class="name">Duplicates</span>`
2. The three group headers. The chevron span keeps its class and becomes an icon holder, and the group gains its own icon:
   - `<span class="chevron">{open.albums ? '▾' : '▸'}</span>` → `<span class="chevron"><Icon name={open.albums ? 'chevron-down' : 'chevron-right'} size={12} /></span><Icon name="folder" size={14} />`
   - the same for `open.people` with `name="user"`, and `open.tags` with `name="tag"`.
3. Every count loses its parentheses, and numbers gain grouping: `({library.info.starredCount})` → `{library.info.starredCount.toLocaleString()}`, and likewise `duplicateCount`, `library.albums.length`, `album.count`, `library.people.length`, `person.count`, `library.tags.length`, `t.count`, `row.count`. (Nine places; `grep -n "class=\"count\"" ` finds them.)

- [ ] **Step 7: `FolderTree.svelte` — styles**

Replace the whole `<style>` block with:

```css
  .tree { display: flex; flex-direction: column; padding: var(--s-2) 0 var(--s-3); }
  /* Rows are inset from the panel edge so the focus ring, which sits 2px outside its
     element, is not clipped by the sidebar's overflow. */
  .root,
  .node,
  .group {
    display: flex;
    align-items: center;
    gap: var(--s-2);
    flex: none;
    height: 28px;
    margin: 0 6px;
    padding: 0 var(--s-2);
    border: 0;
    border-radius: var(--r-3);
    background: none;
    text-align: left;
    cursor: pointer;
    transition: background-color 120ms ease-out;
  }
  .group {
    margin-top: var(--s-2);
    color: var(--text-dim);
    font-size: var(--t-1);
    font-weight: 600;
  }
  .chevron { display: grid; place-items: center; width: 12px; }
  .node { padding-left: 28px; }
  .root:hover, .node:hover, .group:hover { background: var(--hover); }
  .starred.active, .recent.active, .duplicates.active, .node.active { background: var(--accent-soft); }
  /* --text-dim does not reach 4.5:1 over --accent-soft; --text does (tokens.test.ts). */
  .active .count { color: var(--text); }
  .add-album { color: var(--text-dim); }
  .add-album:hover { color: var(--text); }
  .editor {
    height: 28px;
    margin: 0 6px 0 26px;
    padding: 0 var(--s-2);
    border: 1px solid var(--accent);
    border-radius: var(--r-2);
    background: var(--surface);
    color: inherit;
    font: inherit;
  }
  .year {
    margin: var(--s-3) 0 2px;
    padding: 0 14px;
    color: var(--text-dim);
    font-size: var(--t-1);
    font-weight: 600;
    letter-spacing: 0.04em;
  }
  .name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .count {
    margin-left: auto;
    color: var(--text-dim);
    font-size: var(--t-1);
    font-variant-numeric: tabular-nums;
  }
  .empty { padding: var(--s-2) 14px; color: var(--text-dim); }
  .empty.small { margin: 0; padding: 2px 14px 6px 34px; font-size: var(--t-2); }
  .add {
    margin: 0 14px;
    padding: 6px;
    border: 0;
    border-radius: var(--r-3);
    background: var(--field);
    cursor: pointer;
  }
  .add:hover { background: var(--hover); }
  .menu {
    position: fixed;
    z-index: 40;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    padding: var(--s-1);
    background: var(--raised);
    border-radius: var(--r-3);
    /* The hairline is what separates a white menu from a white grid in light mode. */
    box-shadow: 0 0 0 1px var(--line), var(--shadow-menu);
  }
  .menu button {
    padding: 6px 10px;
    border: 0;
    border-radius: var(--r-2);
    background: none;
    text-align: left;
    cursor: pointer;
  }
  .menu button:hover:not(:disabled) { background: var(--hover); }
  .menu button:disabled { color: var(--text-dim); cursor: default; }
  .menu .danger { color: var(--danger); }
  @media (prefers-reduced-motion: reduce) {
    .root, .node, .group { transition: none; }
  }
```

Notes: `.root` loses `font-weight: 600` (spec §5: rows are normal weight; the active row is marked by `--accent-soft`). `flex: none` with a fixed height stops the column flexbox from squeezing rows when the list is long.

- [ ] **Step 8: Check the constraints mechanically**

```bash
cd ui/src && for f in App.svelte components/SearchBar.svelte components/FolderTree.svelte components/StatusBar.svelte; do
  awk '/^<style>/{p=1} p' "$f" | grep -nE '#[0-9a-fA-F]{3,8}\b|rgba?\(|--(bg|panel|panel-2|muted)\b' && echo "LITERAL OR OLD NAME IN $f";
  grep -nP '[⚙★☆🕘⧉▸▾]' "$f" && echo "GLYPH IN $f"; done; echo checked
```

Expected: only `checked`.

- [ ] **Step 9: Gate and commit**

```bash
npm run check && npm test
git add ui/src/app.css ui/src/App.svelte ui/src/components/SearchBar.svelte ui/src/components/FolderTree.svelte ui/src/components/StatusBar.svelte
git commit -m "feat(ui): the shell in the new component language

Top bar, search field, sidebar, splitter and status bar on tokens only, with
icons for the glyphs and counts as right-aligned numerals. Sidebar rows are
inset 6px, which is also what stops the global focus ring clipping at the
panel edge. No handler, attribute or DOM order changes, and no logic, so no
test: the look is on the README smoke checklist.
"
```

(End the message with your session's attribution trailer.)

---

### Task 2: README smoke checks

**Files:**
- Modify: `README.md` (`## Manual smoke checklist`)

- [ ] **Step 1: Add, after the existing theme items and in their style:**

```markdown
- [ ] In Light and in Dark: the top bar, sidebar and status bar are one tinted surface with
      hairline dividers; the active sidebar row is a blue-tinted pill and its count stays
      readable; hovering a row tints it.
- [ ] Sidebar icons (star, clock, copies, the chevrons and the three group icons) are crisp
      and follow the text colour; no emoji or box glyph appears anywhere in the shell.
- [ ] Tab through the sidebar: the focus ring shows whole on every row, not clipped at the
      panel's edge. Drag the splitter, and resize it with the arrow keys, as before.
- [ ] A long album or folder name ellipsises and its count stays right-aligned at every
      sidebar width.
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs(readme): smoke checks for the restyled shell"
```

(With your session's attribution trailer.)
