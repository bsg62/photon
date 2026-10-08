# The Website in photon's Own Dress: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restyle `site/index.html` and `site/404.html` with the app's real tokens, icons, key caps and a tighter type scale, and regenerate the site's screenshots from a harness that no longer leaves a tile blank.

**Architecture:** The site stays one hand-written HTML page with inline CSS and no JavaScript. A new vitest file reads the two pages as text and holds their tokens and icons to `ui/src/tokens.css` and `ui/src/lib/icons.ts`. The screenshot harness in `xtask` learns to serve a photo's small copy to requests that draw it small, which is what removes the blank tile.

**Tech Stack:** HTML and CSS by hand; vitest (`?raw` imports, node project); Rust (`xtask`); headless Chromium and ImageMagick for the screenshots and the checks.

**Spec:** `docs/superpowers/specs/2026-10-08-photon-site-design-refresh-design.md`

## Global Constraints

- One hand-written page, inline CSS, no JavaScript, no build step. `pages.yml` uploads `site/` as it is.
- No request to any other host: no web font, no analytics, no icon CDN.
- No sentence is added, removed or reordered. Two exceptions, both in the spec: keys become `<kbd>`, and the footer gains "Icons from Lucide".
- Light and dark follow `prefers-color-scheme`. There is no switch.
- Tokens are the app's, value for value: `--surface`, `--field`, `--line`, `--text`, `--text-dim`, `--accent`, `--on-accent`. `--shadow` is the page's own. `--chrome` goes.
- The system font stack stays: `system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif`.
- Not in this: a top bar, navigation, bordered cards, a Light/Dark control, more screenshots, a second page, new wording.
- Project conventions (CLAUDE.md): a new test is shown to fail with its change reverted; the GUI is never launched to verify; every commit message ends with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- Work on the branch `site-design-refresh`, which already holds the spec.

## Review Focus

1. **A narrow phone.** A run of `<code>` or `<kbd>` that cannot break makes the page scroll sideways at 320px. Expected: no horizontal overflow at 320px or 390px. Checked in Task 6 by measuring `scrollWidth` in headless Chromium.
2. **A `var(--x)` the page reads and never declares**, such as a leftover `var(--chrome)`. The declaration silently computes to nothing. Pinned in Task 1's test.
3. **A token declared for light and not for dark.** Dark then shows the light value. Pinned in Task 1's test.
4. **A heading whose `<use>` names a symbol that is not there.** The icon is simply absent, with no error. Pinned in Task 2's test.
5. **Icon path data that differs from `icons.ts`.** It was copied by hand and draws a different icon than the app's. Pinned in Task 2's test.

---

## File Structure

| File | Change | Responsibility |
|---|---|---|
| `ui/src/lib/site.test.ts` | Create | Holds the site's tokens and icons to the app's |
| `site/index.html` | Modify | The stylesheet (Task 1), the markup (Task 2) |
| `site/404.html` | Modify | The same tokens, type and focus ring (Task 3) |
| `crates/xtask/src/screenshots.rs` | Modify | Serve a photo's small copy to a tile and a face crop (Task 4) |
| `crates/xtask/screenshots/photos/thumbs/03.jpg` | Create | The small copy of the one large photo (Task 4) |
| `crates/xtask/screenshots/photos/CREDITS.md`, `CLAUDE.md` | Modify | Say what `thumbs/` is for (Task 4) |
| `site/img/main-light.webp`, `main-dark.webp`, `viewer-info-light.webp`, `og.jpg` | Regenerate | Task 5 |

---

### Task 1: The page's stylesheet, held to the app's tokens

**Files:**
- Create: `ui/src/lib/site.test.ts`
- Modify: `site/index.html` (the whole `<style>` block, lines 14-86)

**Interfaces:**
- Produces: `site.test.ts` with the helpers `pageCss(html)`, `pageThemes(html)` and `appTheme(selector)`, which Task 2 and Task 3 extend. The stylesheet defines the classes `.icon` and `.sprite` and the element style `kbd`, which Task 2's markup uses.

- [ ] **Step 1: Write the failing test**

Create `ui/src/lib/site.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import tokens from '../tokens.css?raw';
import index from '../../../site/index.html?raw';
import notFound from '../../../site/404.html?raw';

// The website (site/) is hand-written and has no build step, so nothing shares a line of CSS
// with the app. Its stylesheet says its palette is the app's; this file is what holds it to
// that. It had drifted before: two tokens were approximations nobody had compared.

type Tokens = Record<string, string>;

/** The page's tokens that are its own and have no counterpart in tokens.css. */
const OWN = ['--shadow'];

const PAGES: [string, string][] = [
  ['index.html', index],
  ['404.html', notFound],
];

const bare = (css: string) => css.replace(/\/\*[\s\S]*?\*\//g, '');

function props(body: string): Tokens {
  const out: Tokens = {};
  for (const decl of body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) out[decl[1]] = decl[2].trim();
  return out;
}

/** The app's tokens for one theme: every rule of tokens.css whose selector list holds `selector`. */
function appTheme(selector: string): Tokens {
  const out: Tokens = {};
  for (const rule of bare(tokens).matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    const selectors = rule[1].split(',').map((s) => s.trim());
    if (selectors.includes(selector)) Object.assign(out, props(rule[2]));
  }
  return out;
}

/** A page's stylesheet, without its comments. */
function pageCss(html: string): string {
  const style = /<style>([\s\S]*?)<\/style>/.exec(html);
  if (!style) throw new Error('the page has no <style> block');
  return bare(style[1]);
}

/** A page's own tokens: its `:root` block outside any media query, and the one inside
 *  `prefers-color-scheme: dark`. */
function pageThemes(html: string): { light: Tokens; dark: Tokens } {
  const css = pageCss(html);
  const dark = /@media \(prefers-color-scheme: dark\)\s*\{\s*:root\s*\{([^}]*)\}\s*\}/.exec(css);
  if (!dark) throw new Error('the page has no dark :root block');
  const light = /:root\s*\{([^}]*)\}/.exec(css.replace(dark[0], ''));
  if (!light) throw new Error('the page has no light :root block');
  return { light: props(light[1]), dark: props(dark[1]) };
}

describe.each(PAGES)('site/%s', (_name, html) => {
  const page = pageThemes(html);

  it.each([
    ['light', "[data-theme='light']"],
    ['dark', "[data-theme='dark']"],
  ] as const)("declares the app's %s tokens, value for value", (theme, selector) => {
    const app = appTheme(selector);
    const shared = Object.entries(page[theme]).filter(([name]) => !OWN.includes(name));
    expect(shared.length).toBeGreaterThan(0);
    // A token tokens.css does not have compares against undefined, and fails by name.
    for (const [name, value] of shared) expect(value, `${name} (${theme})`).toBe(app[name]);
  });

  it('declares every token for both themes', () => {
    // One missing from the dark block is not an error anywhere: dark shows the light value.
    expect(Object.keys(page.dark).sort()).toEqual(Object.keys(page.light).sort());
  });

  it('reads no token it does not declare', () => {
    // `var(--chrome)` left behind after the token went computes to nothing, silently.
    const read = new Set([...pageCss(html).matchAll(/var\((--[\w-]+)\)/g)].map((m) => m[1]));
    for (const name of read) expect(Object.keys(page.light), name).toContain(name);
  });
});
```

- [ ] **Step 2: Run the test and see it fail**

Run: `npm test -w ui -- src/lib/site.test.ts`

Expected: FAIL for `site/index.html`, with `--line (light): expected '#d9d9de' to be '#00000018'` and `--line (dark): expected '#3a3a40' to be '#ffffff14'`. The `site/404.html` tests pass: its four tokens already match.

- [ ] **Step 3: Replace the stylesheet**

In `site/index.html`, replace everything from `<style>` to `</style>` (lines 14-86) with:

```html
<style>
  /* The app's own tokens (ui/src/tokens.css), value for value, so the page and the
     screenshots agree; ui/src/lib/site.test.ts fails when one differs. --shadow is the
     page's own. */
  :root {
    color-scheme: light dark;
    --surface: #ffffff;
    --field: #0000000d;
    --line: #00000018;
    --text: #1f1f23;
    --text-dim: #5f5f67;
    --accent: #1f6fd6;
    --on-accent: #ffffff;
    --shadow: 0 6px 24px #00000022;
  }
  @media (prefers-color-scheme: dark) {
    :root {
      --surface: #1b1b1e;
      --field: #ffffff12;
      --line: #ffffff14;
      --text: #ededf0;
      --text-dim: #a6a6af;
      --accent: #62a0ea;
      --on-accent: #111111;
      --shadow: 0 6px 24px #00000066;
    }
  }
  * { box-sizing: border-box; }
  body {
    margin: 0;
    background: var(--surface);
    color: var(--text);
    font: 16px/1.6 system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif;
  }
  main { max-width: 56rem; margin: 0 auto; padding: 3.5rem 1.5rem 4rem; }
  a { color: var(--accent); text-underline-offset: 2px; }
  :focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 4px; }

  header { text-align: center; }
  header img { width: 84px; height: 84px; }
  h1 { font-size: 2.5rem; line-height: 1.1; font-weight: 700; letter-spacing: -.02em; margin: .25rem 0 0; }
  .tagline { font-size: 1.125rem; color: var(--text-dim); margin: .375rem 0 1.25rem; }
  .button {
    display: inline-block;
    padding: .625rem 1.25rem;
    border-radius: 8px;
    background: var(--accent);
    color: var(--on-accent);
    font-size: .9375rem;
    font-weight: 600;
    text-decoration: none;
  }
  .button.plain { background: var(--field); color: var(--text); margin-left: .25rem; }
  .platforms { color: var(--text-dim); font-size: .8125rem; margin: .75rem 0 0; }

  figure { margin: 2.5rem 0; }
  /* The hairline is a shadow, not a border: a border would change the picture's box. */
  figure img {
    display: block;
    width: 100%;
    height: auto;
    border-radius: 8px;
    box-shadow: 0 0 0 1px var(--line), var(--shadow);
  }
  figcaption { color: var(--text-dim); font-size: .8125rem; text-align: center; margin-top: .75rem; }

  h2 { font-size: 1.375rem; line-height: 1.25; font-weight: 650; letter-spacing: -.01em; margin: 3.5rem 0 .75rem; }
  /* Prose is held to a readable measure; the pictures, the features and the tables use the
     width. */
  main > p, main > ul, .faq > p { max-width: 44rem; }
  p { margin: .75rem 0; }
  ul { padding-left: 1.25rem; }
  li { margin: .375rem 0; }
  li::marker { color: var(--text-dim); }

  .features { display: grid; grid-template-columns: repeat(auto-fit, minmax(15rem, 1fr)); gap: 1.5rem 2rem; margin-top: 1.25rem; }
  .features h3 { display: flex; align-items: flex-start; gap: .5rem; font-size: .9375rem; line-height: 1.35; font-weight: 600; margin: 0 0 .25rem; }
  .features p { margin: 0; color: var(--text-dim); font-size: .90625rem; line-height: 1.55; }
  /* The symbols the icons are drawn from: in the document, taking no room. */
  .sprite { position: absolute; width: 0; height: 0; }
  /* One of the app's icons (ui/src/lib/icons.ts), drawn as Icon.svelte draws it. `color`
     and not `stroke` carries the accent: the tag's dot is filled with currentColor. */
  .icon {
    width: 16px; height: 16px; flex: none; margin-top: .15em;
    color: var(--accent);
    fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; stroke-linejoin: round;
  }

  .faq h3 { font-size: 1rem; font-weight: 600; margin: 1.75rem 0 .25rem; }
  .faq p { margin: .25rem 0; }

  .table-wrap { overflow-x: auto; margin: .75rem 0; }
  table { border-collapse: collapse; width: 100%; font-size: .90625rem; }
  th, td { text-align: left; padding: .5rem .75rem .5rem 0; border-bottom: 1px solid var(--line); vertical-align: top; }
  th { font-size: .75rem; font-weight: 600; color: var(--text-dim); }
  code {
    font: .8125em ui-monospace, 'SF Mono', 'Cascadia Mono', Menlo, Consolas, monospace;
    background: var(--field);
    padding: .1em .35em;
    border-radius: 4px;
  }
  /* A key, drawn as the app's shortcut sheet draws one (ShortcutList.svelte). */
  kbd {
    display: inline-block;
    min-width: 1.6em;
    padding: 0 .4em;
    border-radius: 6px;
    background: var(--field);
    box-shadow: inset 0 0 0 1px var(--line);
    font: inherit;
    font-size: .8125em;
    line-height: 1.5;
    text-align: center;
  }
  footer { border-top: 1px solid var(--line); margin-top: 4rem; padding-top: 1.25rem; color: var(--text-dim); font-size: .8125rem; text-align: center; }
  @media (max-width: 30rem) {
    main { padding: 2.5rem 1rem 3rem; }
    h1 { font-size: 2rem; }
    .button.plain { margin: .5rem 0 0; }
  }
</style>
```

- [ ] **Step 4: Run the test and see it pass**

Run: `npm test -w ui -- src/lib/site.test.ts`

Expected: PASS, 8 tests (4 for each page).

- [ ] **Step 5: Show that each assertion discriminates**

Make each change below in `site/index.html`, run `npm test -w ui -- src/lib/site.test.ts`, confirm the named test fails, and undo the change before the next.

| Change | Test that must fail |
|---|---|
| Light `--accent: #1f6fd6;` to `#1f6fd7;` | declares the app's light tokens |
| Delete the dark block's `--on-accent: #111111;` line | declares every token for both themes |
| In the `th, td` rule, `var(--line)` to `var(--chrome)` | reads no token it does not declare |
| Add `--brand: #ff0000;` to both `:root` blocks | declares the app's light tokens, with `--brand (light)` in the message |

After the last undo run the test once more. Expected: PASS.

- [ ] **Step 6: Typecheck**

Run: `npm run check`

Expected: `0 ERRORS 0 WARNINGS`.

- [ ] **Step 7: Commit**

```bash
git add ui/src/lib/site.test.ts site/index.html
git commit -m "feat(site): the page's stylesheet takes the app's tokens, and a test holds it to them"
```

End the message with the `Co-Authored-By` line from Global Constraints. In the body, list the four probes of Step 5 and that each failed.

---

### Task 2: Icons, key caps and the icon credit

**Files:**
- Modify: `site/index.html` (markup only)
- Modify: `ui/src/lib/site.test.ts`

**Interfaces:**
- Consumes: the `.sprite` and `.icon` classes and the `kbd` style from Task 1; `ICONS` from `ui/src/lib/icons.ts` (`Record<IconName, string>`, each value the inside of the icon's `<svg>`).
- Produces: nothing later tasks use.

- [ ] **Step 1: Write the failing tests**

In `ui/src/lib/site.test.ts`, add this import under the existing ones:

```ts
import { ICONS } from './icons';
```

and append at the end of the file:

```ts
describe('the feature icons of site/index.html', () => {
  const symbols = new Map(
    [...index.matchAll(/<symbol id="i-([\w-]+)" viewBox="0 0 24 24">([\s\S]*?)<\/symbol>/g)].map((m) => [m[1], m[2]]),
  );
  const from = index.indexOf('<div class="features">');
  const features = index.slice(from, index.indexOf('<figure>', from));
  const headings = [...features.matchAll(/<h3>([\s\S]*?)<\/h3>/g)].map((m) => m[1]);
  const used = headings.map((h) => /^<svg class="icon" aria-hidden="true"><use href="#i-([\w-]+)"\/><\/svg>/.exec(h)?.[1]);

  it('gives every feature one icon, and no two the same', () => {
    expect(headings.length).toBe(9);
    expect(used.every((name) => name !== undefined)).toBe(true);
    expect(new Set(used).size).toBe(9);
  });

  it('draws every icon from a symbol the page holds, and holds no other', () => {
    // A <use> naming a symbol that is not there draws nothing and reports nothing.
    expect([...symbols.keys()].sort()).toEqual([...used].sort());
  });

  it("copies each symbol from the app's icon, path for path", () => {
    expect(symbols.size).toBe(9);
    for (const [name, body] of symbols) expect(body, name).toBe((ICONS as Record<string, string>)[name]);
  });
});
```

- [ ] **Step 2: Run the tests and see them fail**

Run: `npm test -w ui -- src/lib/site.test.ts`

Expected: the three new tests FAIL (`expected false to be true` for the icons, `expected 0 to be 9` for the symbols). The eight from Task 1 pass.

- [ ] **Step 3: Add the symbols**

Print the nine symbols from the app's icon data, so nothing is retyped:

```bash
python3 - <<'EOF'
import re
src = open('ui/src/lib/icons.ts').read()
for name in ['star', 'layout-grid', 'folder', 'search', 'tag', 'user', 'copy', 'eye-off', 'crop']:
    m = re.search(r"^\s+'?%s'?:\s*\n?\s*'([^']+)'," % re.escape(name), src, re.M)
    print('  <symbol id="i-%s" viewBox="0 0 24 24">%s</symbol>' % (name, m.group(1)))
EOF
```

In `site/index.html`, directly after the `<body>` line, insert the block below, with the script's nine lines in place of the line `NINE SYMBOLS`:

```html
<!-- The feature icons are from Lucide (https://lucide.dev), lucide-static 1.47.0, unmodified:
     ISC License, Copyright (c) 2026 Lucide Icons and Contributors. `search` derives from
     Feather: MIT License, Copyright (c) 2013-present Cole Bemis. Both licences in full:
     https://github.com/bsg62/photon/blob/main/THIRD-PARTY-NOTICES.md
     Copied from ui/src/lib/icons.ts; ui/src/lib/site.test.ts fails when one differs. -->
<svg class="sprite" aria-hidden="true">
NINE SYMBOLS
</svg>
```

- [ ] **Step 4: Put an icon before each feature heading**

Each replacement is an exact string that occurs once in `site/index.html`:

| Find | Replace with |
|---|---|
| `<h3>Picks up where Picasa left off</h3>` | `<h3><svg class="icon" aria-hidden="true"><use href="#i-star"/></svg>Picks up where Picasa left off</h3>` |
| `<h3>Fast with big libraries</h3>` | `<h3><svg class="icon" aria-hidden="true"><use href="#i-layout-grid"/></svg>Fast with big libraries</h3>` |
| `<h3>Reorganise in your file manager</h3>` | `<h3><svg class="icon" aria-hidden="true"><use href="#i-folder"/></svg>Reorganise in your file manager</h3>` |
| `<h3>Search that knows your photos</h3>` | `<h3><svg class="icon" aria-hidden="true"><use href="#i-search"/></svg>Search that knows your photos</h3>` |
| `<h3>Albums and keywords</h3>` | `<h3><svg class="icon" aria-hidden="true"><use href="#i-tag"/></svg>Albums and keywords</h3>` |
| `<h3>People</h3>` | `<h3><svg class="icon" aria-hidden="true"><use href="#i-user"/></svg>People</h3>` |
| `<h3>Duplicates and look-alikes</h3>` | `<h3><svg class="icon" aria-hidden="true"><use href="#i-copy"/></svg>Duplicates and look-alikes</h3>` |
| `<h3>Hides, never deletes</h3>` | `<h3><svg class="icon" aria-hidden="true"><use href="#i-eye-off"/></svg>Hides, never deletes</h3>` |
| `<h3>Rotate, crop, export</h3>` | `<h3><svg class="icon" aria-hidden="true"><use href="#i-crop"/></svg>Rotate, crop, export</h3>` |

- [ ] **Step 5: Draw the keys as key caps**

Exact strings again, each occurring once. Line breaks inside a string are the file's own.

| Find | Replace with |
|---|---|
| `away with <code>H</code> or a right-click` | `away with <kbd>H</kbd> or a right-click` |
| `double-click, <code>+</code> and` | `double-click, <kbd>+</kbd> and` |
| `<code>-</code>, or Ctrl and the wheel.` | `<kbd>-</kbd>, or <kbd>Ctrl</kbd> and the wheel.` |
| `with Ctrl+C (⌘C on a Mac)` | `with <kbd>Ctrl</kbd>+<kbd>C</kbd> (<kbd>⌘</kbd><kbd>C</kbd> on a Mac)` |
| `<code>?</code> lists them all, <code>.</code>` | `<kbd>?</kbd> lists them all, <kbd>.</kbd>` |
| `stars, <code>H</code> hides, and Shift with the arrows` | `stars, <kbd>H</kbd> hides, and <kbd>Shift</kbd> with the arrows` |
| `out of the way with Ctrl+B.` | `out of the way with <kbd>Ctrl</kbd>+<kbd>B</kbd>.` |

Leave `The <code>?</code> in the box lists every term` in the search feature as it is: that is the button in the search box, not a key.

- [ ] **Step 6: Credit the icons in the footer**

Find:

```html
    <a href="https://github.com/bsg62/photon/blob/main/crates/xtask/screenshots/photos/CREDITS.md">Wikimedia Commons</a>.
```

Replace with:

```html
    <a href="https://github.com/bsg62/photon/blob/main/crates/xtask/screenshots/photos/CREDITS.md">Wikimedia Commons</a>.
    Icons from <a href="https://github.com/bsg62/photon/blob/main/THIRD-PARTY-NOTICES.md">Lucide</a>.
```

- [ ] **Step 7: Run the tests and see them pass**

Run: `npm test -w ui -- src/lib/site.test.ts`

Expected: PASS, 11 tests.

- [ ] **Step 8: Show that each new assertion discriminates**

Make each change in `site/index.html`, run the test, confirm the named failure, undo.

| Change | Test that must fail |
|---|---|
| In the People heading, `#i-user` to `#i-usr` | draws every icon from a symbol the page holds |
| In the Duplicates heading, `#i-copy` to `#i-crop` | gives every feature one icon, and no two the same |
| In the `i-crop` symbol, `M6 2v14` to `M6 2v15` | copies each symbol from the app's icon |

After the last undo run the test once more. Expected: PASS.

- [ ] **Step 9: Check that no sentence changed**

Save this as `words.py` in the session's scratchpad directory (not in the repository) and run
it from the repository root with `python3 <scratchpad>/words.py`:

```python
import re
import subprocess


def words(html):
    body = html[html.index("<body>"):]
    body = re.sub(r"<!--.*?-->", "", body, flags=re.S)
    body = re.sub(r'<svg class="sprite".*?</svg>', "", body, flags=re.S)
    return re.sub(r"\s+", " ", re.sub(r"<[^>]*>", "", body)).strip()


shown = subprocess.run(["git", "show", "main:site/index.html"], capture_output=True, text=True, check=True)
old = words(shown.stdout)
new = words(open("site/index.html").read())
credited = old.replace("Wikimedia Commons.", "Wikimedia Commons. Icons from Lucide.")
print("the same words, but for the credit" if new == credited else "DIFFERENT")
```

Expected: `the same words, but for the credit`. `DIFFERENT` means a sentence changed besides the
footer's added "Icons from Lucide.": print `old` and `new`, find it, and fix it.

- [ ] **Step 10: Commit**

```bash
git add site/index.html ui/src/lib/site.test.ts
git commit -m "feat(site): the app's icons before the features, and keys drawn as key caps"
```

End the message with the `Co-Authored-By` line. In the body, list the three probes of Step 8.

---

### Task 3: The 404 page

**Files:**
- Modify: `site/404.html` (the `<style>` block)

**Interfaces:**
- Consumes: Task 1's tests, which already read `site/404.html`.
- Produces: nothing.

- [ ] **Step 1: Replace the stylesheet**

In `site/404.html`, replace everything from `<style>` to `</style>` with:

```html
<style>
  /* The app's own tokens, as in index.html; ui/src/lib/site.test.ts holds both pages to them. */
  :root {
    color-scheme: light dark;
    --surface: #ffffff;
    --text: #1f1f23;
    --text-dim: #5f5f67;
    --accent: #1f6fd6;
  }
  @media (prefers-color-scheme: dark) {
    :root {
      --surface: #1b1b1e;
      --text: #ededf0;
      --text-dim: #a6a6af;
      --accent: #62a0ea;
    }
  }
  body {
    margin: 0;
    background: var(--surface);
    color: var(--text);
    font: 16px/1.6 system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif;
  }
  main { max-width: 30rem; margin: 0 auto; padding: 6rem 1rem; text-align: center; }
  img { width: 72px; height: 72px; }
  h1 { font-size: 1.75rem; line-height: 1.1; font-weight: 700; letter-spacing: -.02em; margin: .5rem 0; }
  p { color: var(--text-dim); }
  a { color: var(--accent); text-underline-offset: 2px; }
  :focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 4px; }
</style>
```

The markup below it does not change.

- [ ] **Step 2: Run the tests**

Run: `npm test -w ui -- src/lib/site.test.ts`

Expected: PASS, 11 tests.

- [ ] **Step 3: Show that the tests read this page**

The 404 page's tokens matched the app's before this task, so its tests have never been seen to fail. In `site/404.html` change the dark `--accent: #62a0ea;` to `#62a0eb;` and run the test.

Expected: FAIL in `site/404.html > declares the app's dark tokens`, naming `--accent (dark)`. Undo, run again. Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add site/404.html
git commit -m "feat(site): the 404 page in the same type and focus ring"
```

End the message with the `Co-Authored-By` line, and say in the body that Step 3's probe failed as expected.

---

### Task 4: The screenshot harness serves a small photo to a tile

The dark main-window screenshot on the live site has a blank third tile. Measured on 2026-10-08 with `--photos crates/xtask/screenshots/photos --only main-dark --no-build`: the tile was blank in 5 of 70 runs. `03.jpg` is the one photo kept at 1600x1200 (the viewer shots open it); every other is 480px wide, and no other tile was ever blank. With `03.jpg` shrunk to 480px wide in a copy of the folder, 0 of 40 runs were blank. A tile's `<img>` is `decoding="async"`, and Chromium's `--virtual-time-budget` does not wait for an image decode, so the large photo sometimes is not painted when the shot is taken.

The fix is what the app itself does: a grid tile and a face crop are given a small picture. The harness serves `thumbs/<name>` beside a photo, when that file exists, to `/thumb/<id>/grid/...` and `/face/...`. The viewer's preview (`/thumb/<id>/preview/...`) and the full image (`/image/<id>`) stay the photo itself.

**Files:**
- Modify: `crates/xtask/src/screenshots.rs` (`media` at line 385, `respond` at line 416, the tests module)
- Create: `crates/xtask/screenshots/photos/thumbs/03.jpg`
- Modify: `crates/xtask/screenshots/photos/CREDITS.md`, `CLAUDE.md`

**Interfaces:**
- Consumes: `respond(path: &str, dist: &Path, photos: &[PathBuf]) -> Response`, `list_photos(dir: &Path) -> Result<Vec<PathBuf>, String>` and the test helper `temp_dist(files: &[(&str, &str)]) -> PathBuf`, all existing. `temp_dist` creates parent directories, so a name may be `thumbs/03.jpg`.
- Produces: `media(id: u64, photos: &[PathBuf], small: bool) -> Response` and `small_copy(photo: &Path) -> Option<PathBuf>`, both private. `respond`'s signature is unchanged.

- [ ] **Step 1: Write the failing test**

In `crates/xtask/src/screenshots.rs`, in the tests module, after `photos_are_served_by_item_id_in_name_order_and_wrap_round`, add:

```rust
    #[test]
    fn a_tile_and_a_face_crop_are_given_the_photos_small_copy_when_it_has_one() {
        // 03 has a small copy and 01 has none. Sorted, 01 is item 1 and 03 is item 2.
        let dir = temp_dist(&[
            ("01.jpg", "large 01"),
            ("03.jpg", "large 03"),
            ("thumbs/03.jpg", "small 03"),
        ]);
        let photos = list_photos(&dir).unwrap();
        assert_eq!(photos.len(), 2, "a small copy is not a photo of its own");
        let body = |path: &str| String::from_utf8(respond(path, &dir, &photos).body).unwrap();
        assert_eq!(body("/thumb/2/grid/k1"), "small 03");
        assert_eq!(body("/face/2/k1/144"), "small 03");
        // What the viewer shows is the photo itself.
        assert_eq!(body("/thumb/2/preview/k1"), "large 03");
        assert_eq!(body("/image/2"), "large 03");
        // And a photo with no small copy is served as it is.
        assert_eq!(body("/thumb/1/grid/k0"), "large 01");
        assert_eq!(body("/face/1/k0/144"), "large 01");
    }
```

- [ ] **Step 2: Run the test and see it fail**

Run: `cargo test -p xtask a_tile_and_a_face_crop`

Expected: FAIL at the first `body` assertion: left `"large 03"`, right `"small 03"`.

- [ ] **Step 3: Serve the small copy**

Replace the `media` function (its doc comment included) with:

```rust
/// A photo's small copy: `thumbs/<name>` beside it, if there is one.
fn small_copy(photo: &Path) -> Option<PathBuf> {
    let small = photo.parent()?.join("thumbs").join(photo.file_name()?);
    small.is_file().then_some(small)
}

/// A photo from `--photos` for item `id`, wrapping round after the last; the gradient when
/// none were given. Real photos are for the project website, where a grid of gradients says
/// nothing about a photo manager. `small` is a request that draws the photo small, a grid
/// tile or a face crop, and is given the photo's small copy when it has one: a tile's
/// `<img>` decodes off the main thread and `--virtual-time-budget` does not wait for that,
/// so a 1600px photo in a tile was not painted yet in about one shot in fourteen, and the
/// website's dark screenshot went out with a blank tile.
fn media(id: u64, photos: &[PathBuf], small: bool) -> Response {
    let Some(len) = u64::try_from(photos.len()).ok().filter(|&n| n > 0) else {
        return Response::ok("image/svg+xml", placeholder_svg(id));
    };
    let index = usize::try_from(id.saturating_sub(1) % len).unwrap_or_default();
    let photo = photos[index].as_path();
    let file = small.then(|| small_copy(photo)).flatten();
    match std::fs::read(file.as_deref().unwrap_or(photo)) {
        Ok(body) => Response::ok("image/jpeg", body),
        Err(_) => Response::not_found(),
    }
}
```

In `respond`, replace the line `return media(id, photos);` with:

```rust
        // Drawn small: a grid tile and a face crop. The viewer's preview and the full image
        // are the photo itself.
        let small = path.starts_with("/face/")
            || (path.starts_with("/thumb/") && rest.split('/').nth(1) == Some("grid"));
        return media(id, photos, small);
```

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo test -p xtask`

Expected: PASS, the new test and every existing one.

- [ ] **Step 5: Show that each half of the rule discriminates**

Make each change, run `cargo test -p xtask a_tile_and_a_face_crop`, confirm the failure, undo.

| Change in `respond` | Assertion that must fail |
|---|---|
| `let small = true;` in place of the expression | `body("/thumb/2/preview/k1")` is `"large 03"` |
| Drop `path.starts_with("/face/") \|\|` | `body("/face/2/k1/144")` is `"small 03"` |
| `Some("grid")` to `Some("preview")` | `body("/thumb/2/grid/k1")` is `"small 03"` |

After the last undo, save the file again (so cargo rebuilds it) and run `cargo test -p xtask`. Expected: PASS.

- [ ] **Step 6: Add the small copy**

```bash
mkdir -p crates/xtask/screenshots/photos/thumbs
magick crates/xtask/screenshots/photos/03.jpg -resize 480x -strip -quality 85 crates/xtask/screenshots/photos/thumbs/03.jpg
magick identify -format '%wx%h %b\n' crates/xtask/screenshots/photos/thumbs/03.jpg
```

Expected: `480x360` and a size of roughly 30-40 kB.

- [ ] **Step 7: Check the blank tile is gone**

```bash
cargo run -q -p xtask -- screenshots --photos crates/xtask/screenshots/photos --only main-dark
blank=0
for i in $(seq 1 40); do
  ./target/debug/xtask screenshots --photos crates/xtask/screenshots/photos --only main-dark --no-build >/dev/null 2>&1
  sd=$(magick target/screenshots/main-dark.png -crop 100x100+700+120 +repage -format '%[fx:standard_deviation]' info:)
  case $sd in 0|0.00*|0.01*) blank=$((blank+1));; esac
done
echo "blank: $blank of 40"
```

The crop is a square inside the third tile; a blank tile is one flat colour, a standard deviation under 0.02. Expected: `blank: 0 of 40`. Before this task the same loop gave 2 of 40. If any run is blank, stop: the cause is not what this task says it is, and it is found (superpowers:systematic-debugging) before the screenshots are regenerated.

- [ ] **Step 8: Say what `thumbs/` is**

In `crates/xtask/screenshots/photos/CREDITS.md`, replace:

```
(`mock.js`, Anna) falls on the person in it; the rest are shrunk to thumbnail size.
```

with:

```
(`mock.js`, Anna) falls on the person in it; the rest are shrunk to thumbnail size.
`thumbs/03.jpg` is that photo at thumbnail size (`magick 03.jpg -resize 480x -strip -quality 85`):
a grid tile and a face crop are served the copy in `thumbs/` when a photo has one, because a
1600px photo in a tile was sometimes not decoded when the screenshot was taken.
```

In `CLAUDE.md`, in the paragraph that begins "The project website (`site/`, deployed to photon.webcodr.io by `pages.yml`)", append this sentence at the end of the paragraph:

```
A photo kept large for the viewer shots has a small copy in `photos/thumbs/`, which the
server gives to a grid tile and a face crop: a tile's `<img>` decodes off the main thread and
the virtual-time budget does not wait for it, so the 1600px photo was blank in about one
`main-dark` in fourteen, the website's own among them.
```

- [ ] **Step 9: The Rust gate**

```bash
cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```

Expected: all pass.

- [ ] **Step 10: Commit**

```bash
git add crates/xtask/src/screenshots.rs crates/xtask/screenshots/photos/thumbs/03.jpg crates/xtask/screenshots/photos/CREDITS.md CLAUDE.md
git commit -m "fix(xtask): a tile is served a photo's small copy, so the large one is never blank in a screenshot"
```

End the message with the `Co-Authored-By` line. In the body give the measurements (5 of 70 blank before, 0 of 40 with the photo shrunk, 0 of 40 after), the three probes of Step 5, and that the blank tile itself is a race no unit test can reach: the test holds the rule that removes it, and Step 7's loop is the evidence that it does.

---

### Task 5: The site's screenshots, regenerated

**Files:**
- Regenerate: `site/img/main-light.webp`, `site/img/main-dark.webp`, `site/img/viewer-info-light.webp`, `site/img/og.jpg`

**Interfaces:**
- Consumes: Task 4's harness and `thumbs/03.jpg`.
- Produces: nothing.

- [ ] **Step 1: Take the three shots**

```bash
cargo run -q -p xtask -- screenshots --photos crates/xtask/screenshots/photos --only main-light
for s in main-dark viewer-info-light; do
  ./target/debug/xtask screenshots --photos crates/xtask/screenshots/photos --only $s --no-build
done
magick identify -format '%f %wx%h\n' target/screenshots/main-light.png target/screenshots/main-dark.png target/screenshots/viewer-info-light.png
```

Expected: three files, each `1280x800`.

- [ ] **Step 2: Look at each one**

Open the three PNGs with the Read tool. Check, in each main-window shot: every tile in view holds a photo, the third tile of the first row among them; the sidebar and the top bar are drawn. In the viewer shot: the photo is drawn, the info panel is open beside it, and one outline with the name Anna lies on the person. A shot that fails this is taken again once; if it fails twice, stop and find out why before going on.

- [ ] **Step 3: Convert them**

```bash
for s in main-light main-dark viewer-info-light; do
  magick target/screenshots/$s.png -quality 85 site/img/$s.webp
done
magick target/screenshots/main-light.png -quality 88 site/img/og.jpg
ls -la site/img/
```

Expected: each `.webp` between roughly 100 kB and 210 kB and `og.jpg` near 290 kB, as before; a file several times larger or smaller means a conversion went wrong.

- [ ] **Step 4: Commit**

```bash
git add site/img/main-light.webp site/img/main-dark.webp site/img/viewer-info-light.webp site/img/og.jpg
git commit -m "chore(site): screenshots without the blank tile"
```

End the message with the `Co-Authored-By` line.

---

### Task 6: The page, looked at

Nothing renders the site in a test. This task is the check that replaces one, and it changes no file unless it finds something.

**Files:**
- None, unless a check fails.

**Interfaces:**
- Consumes: the finished `site/index.html` and `site/404.html`.

- [ ] **Step 1: Render both pages, both themes, two widths**

Use the session's scratchpad directory for the output (`$S` below).

```bash
P=file://$PWD/site
chromium --headless --hide-scrollbars --window-size=1280,5200 --screenshot=$S/index-light.png $P/index.html
chromium --headless --hide-scrollbars --force-dark-mode --window-size=1280,5200 --screenshot=$S/index-dark.png $P/index.html
chromium --headless --hide-scrollbars --window-size=390,7600 --screenshot=$S/index-phone.png $P/index.html
chromium --headless --hide-scrollbars --window-size=800,500 --screenshot=$S/404-light.png $P/404.html
chromium --headless --hide-scrollbars --force-dark-mode --window-size=800,500 --screenshot=$S/404-dark.png $P/404.html
```

`--force-dark-mode` sets `prefers-color-scheme: dark` in headless Chromium. On `file://` the 404 page's `/img/icon.svg` does not load; that is the path it has on the live site and is not a defect.

- [ ] **Step 2: Look at them**

Open each PNG with the Read tool (crop tall ones into pieces with `magick x.png -crop 1280x1750+0+N +repage`). Check:

- The nine features stand in three rows of three at 1280px and in one column at 390px, each heading with its icon in the accent colour.
- The paragraphs under "Your photos stay where they are" are narrower than the screenshot above them.
- In dark, the Download button's text is dark on the light blue, and the dark main-window screenshot is the one shown.
- Every key in the "Also" list is a key cap; the search terms are code chips.
- The footer ends with "Icons from Lucide."
- The 404 page is centred and readable in both themes.

- [ ] **Step 3: Measure the overflow on a phone**

```bash
for w in 320 390; do
  sed 's#</body>#<script>document.title = document.documentElement.scrollWidth + " of " + window.innerWidth</script></body>#' site/index.html > site/zz-measure.html
  chromium --headless --window-size=$w,900 --dump-dom file://$PWD/site/zz-measure.html | grep -o '<title>[^<]*</title>'
done
rm site/zz-measure.html
git status --short
```

Expected: `<title>320 of 320</title>` and `<title>390 of 390</title>`, and a clean `git status`. A first number larger than the second is horizontal overflow: find the element (a long `<code>` is the likely one), fix it in the stylesheet with `overflow-wrap: anywhere` on that element, and run Step 1 to Step 3 again. If headless Chromium reports a window wider than asked for (an inner width of 500 at `--window-size=320`), say so in the report and take the 390 result as the measurement.

- [ ] **Step 4: The UI gate**

```bash
npm run check
npm test
```

Expected: `0 ERRORS 0 WARNINGS`, and every test passing.

- [ ] **Step 5: Commit anything Step 3 changed**

Only if a fix was made:

```bash
git add site/index.html
git commit -m "fix(site): no sideways scroll on a narrow phone"
```

End the message with the `Co-Authored-By` line, and name the element and the width in the body.
