# UI Reskin, PR 1 of 5: Foundations — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the token system, the light/dark theme with its System/Light/Dark setting, and the vendored icon component, while leaving every existing component's markup and styles alone.

**Architecture:** `tokens.css` defines semantic tokens under `[data-theme]` blocks and aliases the eight old variable names onto them, so untouched components keep working and already follow the theme. The choice is a row in the existing `settings` table, reached through the usual three-file IPC; a `createTheme` factory with injected dependencies holds the logic, and a render-blocking `theme-boot.js` sets the theme before first paint. Icons are Lucide path data in one typed map behind one `Icon.svelte`.

**Tech Stack:** Rust (rusqlite, serde), Tauri 2, Svelte 5 runes, TypeScript, vitest (node environment).

**Spec:** `docs/superpowers/specs/2026-09-19-photon-ui-reskin-design.md` — read §2, §3, §4 and §7 before starting. This plan covers the spec's PR 1 only. PRs 2–5 (shell, grid, viewer, settings and cleanup) each get their own plan once this one has merged, because they restyle against the tokens and `Icon` this PR delivers.

## Global Constraints

- Branch: `feat/ui-reskin-foundations`, off `main`. One PR. **No release is cut until PR 5 has merged**: after this PR light mode is rough, since components still hold dark-only colour literals.
- **The Rust gate, all before any commit that touches Rust:** `cargo fmt --all`, then `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`.
- **The UI gate, from the repo root:** `npm run check` (0 errors **and** 0 warnings), `npm test`.
- **Never launch the GUI** (`npm run dev`) to verify. Verification is the suites plus `svelte-check`; what needs eyes goes on the README's `## Manual smoke checklist`.
- **Every new test is demonstrated to fail with its change reverted**, using the exact probe written in the task, then the probe is undone. A compile error is not proof. Where a change cannot have a test, the commit message says so and why.
- No new runtime dependency, npm or cargo. No native libraries.
- No schema change: the theme is a row in the existing `settings` table. Do not touch `MIGRATIONS`.
- `ui/src/lib/api.ts` mirrors Rust by hand and nothing checks it: the Rust command and its TS mirror land in the same commit.
- Do not change `TILE`, `GAP` or the header height in `ui/src/lib/layout.ts`.
- Do not edit any existing component's `<style>` block or replace any glyph in this PR, apart from the Appearance section added to `Settings.svelte`. The root font size stays 14px here; it moves to the type scale in PR 2.
- New code contains no colour literal outside `tokens.css`.
- Comments carry reasoning, not mechanics; match the surrounding density.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/photon-core/src/library/settings.rs` (modify) | `ThemeChoice`, `Library::theme`, `Library::set_theme` |
| `crates/photon-core/src/library/mod.rs` (modify) | re-export `ThemeChoice` |
| `crates/photon-app/src/commands.rs`, `ipc.rs`, `app.rs` (modify) | `theme` / `set_theme` commands |
| `crates/photon-app/capabilities/default.json` (modify) | `core:window:allow-set-theme` |
| `ui/src/lib/api.ts` (modify) | `ThemeChoice`, `api.theme`, `api.setTheme`, `api.setWindowTheme` |
| `ui/src/tokens.css` (create) | every colour and scale token; the aliases; the global focus rule |
| `ui/src/app.css` (modify) | imports `tokens.css`; loses its own variables |
| `ui/src/lib/tokens.test.ts` (create) | contrast rule and light/dark parity over `tokens.css` |
| `ui/vite.config.ts` (modify) | `test.css: true`, so a CSS `?raw` import is not empty |
| `ui/src/lib/theme.svelte.ts` (create) | `resolveTheme`, `createTheme` — pure, dependency-injected |
| `ui/src/lib/theme.svelte.test.ts` (create) | its tests |
| `ui/src/lib/app-theme.svelte.ts` (create) | the production singleton: DOM, `localStorage`, Tauri |
| `ui/public/theme-boot.js` (create) | sets `data-theme` before first paint |
| `ui/src/lib/theme-boot.test.ts` (create) | its tests |
| `ui/index.html` (modify) | loads `theme-boot.js` |
| `ui/src/App.svelte` (modify) | `theme.init()` / `dispose()` in `onMount` |
| `ui/src/lib/settings.ts`, `ui/src/components/Settings.svelte` (modify) | the Appearance section |
| `ui/src/lib/icons.ts`, `ui/src/components/Icon.svelte` (create) | the icon set |
| `THIRD-PARTY-NOTICES.md` (create) | Lucide's ISC licence and Feather's MIT licence |
| `README.md` (modify) | smoke checklist entries, Linux note |

---

### Task 1: The theme setting in photon-core

**Files:**
- Modify: `crates/photon-core/src/library/settings.rs`
- Modify: `crates/photon-core/src/library/mod.rs:10-15` (the `pub use` list)

**Interfaces:**
- Consumes: the private `Library::setting(&self, key) -> Result<Option<String>>` and `Library::set_setting(&self, key, value) -> Result<()>` already in `settings.rs`.
- Produces: `photon_core::library::ThemeChoice` — `enum { System, Light, Dark }`, `Copy`, `Default` = `System`, serde lowercase (`"system"`, `"light"`, `"dark"`); `Library::theme(&self) -> Result<ThemeChoice>`; `Library::set_theme(&self, choice: ThemeChoice) -> Result<()>`.

- [ ] **Step 1: Create the branch**

```bash
git switch main && git pull --ff-only && git switch -c feat/ui-reskin-foundations
```

- [ ] **Step 2: Write the failing tests**

In `settings.rs`, inside `mod tests`, after `the_slideshow_interval_defaults_persists_and_is_clamped_both_ways`:

```rust
    #[test]
    fn the_theme_defaults_to_system_and_round_trips() {
        let (_dir, lib) = temp_library();
        assert_eq!(lib.theme().unwrap(), ThemeChoice::System);
        for choice in [ThemeChoice::Dark, ThemeChoice::Light, ThemeChoice::System] {
            lib.set_theme(choice).unwrap();
            assert_eq!(lib.theme().unwrap(), choice);
        }
    }

    #[test]
    fn a_theme_this_photon_does_not_know_reads_as_system() {
        let (_dir, lib) = temp_library();
        lib.set_theme(ThemeChoice::Dark).unwrap();
        // Written by a newer photon, or by hand.
        lib.set_setting(THEME, "sepia").unwrap();
        assert_eq!(lib.theme().unwrap(), ThemeChoice::System);
    }

    #[test]
    fn a_theme_choice_crosses_ipc_in_lowercase() {
        assert_eq!(serde_json::to_string(&ThemeChoice::System).unwrap(), "\"system\"");
        assert_eq!(serde_json::from_str::<ThemeChoice>("\"dark\"").unwrap(), ThemeChoice::Dark);
    }
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo test -p photon-core --lib settings`
Expected: compile errors, `cannot find type ThemeChoice` and `cannot find value THEME`. (A compile error is only the starting point; the discriminating probes are Step 6.)

- [ ] **Step 4: Implement**

In `settings.rs`, add `use serde::{Deserialize, Serialize};` beside the existing imports. After the `SLIDESHOW_INTERVAL_RANGE_S` constant:

```rust
/// Which colour scheme the UI uses.
const THEME: &str = "theme";

/// The user's colour scheme: the desktop's, or one of the two pinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// Anything unrecognised is `System`: the table is plain text a newer photon may have
    /// written, and following the desktop is the one answer that is never wrong.
    fn parse(stored: &str) -> Self {
        match stored {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }
}
```

In `impl Library`, after `set_slideshow_interval_s`:

```rust
    /// The colour scheme the user chose; `System` when never set.
    pub fn theme(&self) -> Result<ThemeChoice> {
        Ok(self
            .setting(THEME)?
            .map_or(ThemeChoice::System, |stored| ThemeChoice::parse(&stored)))
    }

    /// Stores the colour scheme.
    pub fn set_theme(&self, choice: ThemeChoice) -> Result<()> {
        self.set_setting(THEME, choice.as_str())
    }
```

Update the module doc comment's count at the top of the file: "Three things so far" becomes "Four things so far", adding "which colour scheme the UI uses" to the list.

In `library/mod.rs`, add to the `pub use` block, keeping it alphabetical by module:

```rust
pub use settings::ThemeChoice;
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p photon-core --lib settings`
Expected: all pass.

- [ ] **Step 6: Prove each test discriminates**

Apply each probe, run `cargo test -p photon-core --lib settings`, see the named test fail, undo the probe.

1. In `parse`, change `"dark" => Self::Dark` to `"dark" => Self::Light`. Expected: `the_theme_defaults_to_system_and_round_trips` fails.
2. In `parse`, change `_ => Self::System` to `_ => Self::Dark`. Expected: `a_theme_this_photon_does_not_know_reads_as_system` fails (and the default assertion in the first test).
3. Delete the `#[serde(rename_all = "lowercase")]` line. Expected: `a_theme_choice_crosses_ipc_in_lowercase` fails with `"System"`.

Run `git diff --stat` afterwards and confirm only the intended changes remain.

- [ ] **Step 7: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo bench -p photon-core --bench grid --no-run
git add crates/photon-core
git commit -m "feat(settings): a theme choice, stored beside the slideshow interval

System, Light or Dark in the existing settings table, so no schema change. An
unrecognised stored value reads as System. Each test was run against a probe
that breaks the line it covers.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: The IPC commands and their TypeScript mirror

**Files:**
- Modify: `crates/photon-app/src/commands.rs` (after `set_slideshow_interval`, ~line 354)
- Modify: `crates/photon-app/src/ipc.rs` (after `set_slideshow_interval`, ~line 89)
- Modify: `crates/photon-app/src/app.rs` (the `generate_handler!` list, after `ipc::set_slideshow_interval`, ~line 167)
- Modify: `crates/photon-app/capabilities/default.json`
- Modify: `ui/src/lib/api.ts`

**Interfaces:**
- Consumes: `Library::theme`, `Library::set_theme`, `photon_core::library::ThemeChoice` from Task 1.
- Produces, in `api.ts`: `export type ThemeChoice = 'system' | 'light' | 'dark'`; `api.theme(): Promise<ThemeChoice>`; `api.setTheme(choice: ThemeChoice): Promise<void>`; `api.setWindowTheme(theme: 'light' | 'dark' | null): Promise<void>`.

- [ ] **Step 1: `commands.rs`**

After `set_slideshow_interval`:

```rust
/// The colour scheme the user chose.
pub fn theme(engine: &Engine) -> CmdResult<photon_core::library::ThemeChoice> {
    Ok(engine.lib.theme()?)
}

pub fn set_theme(engine: &Engine, choice: photon_core::library::ThemeChoice) -> CmdResult<()> {
    Ok(engine.lib.set_theme(choice)?)
}
```

If `commands.rs` already imports names from `photon_core::library` in a `use` block, add `ThemeChoice` there and use the short name instead.

- [ ] **Step 2: `ipc.rs`**

After `set_slideshow_interval`:

```rust
#[tauri::command(async)]
pub fn theme(engine: Eng<'_>) -> Result<photon_core::library::ThemeChoice, AppError> {
    commands::theme(&engine)
}

#[tauri::command(async)]
pub fn set_theme(engine: Eng<'_>, choice: photon_core::library::ThemeChoice) -> Result<(), AppError> {
    commands::set_theme(&engine, choice)
}
```

- [ ] **Step 3: `app.rs`**

In `tauri::generate_handler![...]`, after `ipc::set_slideshow_interval,`:

```rust
            ipc::theme,
            ipc::set_theme,
```

Forgetting this compiles and fails only at runtime. Confirm with `grep -n "ipc::theme\|ipc::set_theme" crates/photon-app/src/app.rs` — two lines.

- [ ] **Step 4: The capability**

In `capabilities/default.json`, add `"core:window:allow-set-theme"` to `permissions`, after `"core:window:allow-set-fullscreen"`. Without it `setWindowTheme` rejects at runtime, inside the webview.

- [ ] **Step 5: `api.ts`**

Beside the other exported types:

```ts
/** Mirrors `photon_core::library::ThemeChoice` (serde lowercase). */
export type ThemeChoice = 'system' | 'light' | 'dark';
```

In the `api` object, after `setSlideshowInterval`:

```ts
  theme: () => invoke<ThemeChoice>('theme'),
  setTheme: (choice: ThemeChoice) => invoke<void>('set_theme', { choice }),
  /** The native title bar's scheme; null hands it back to the desktop. Granted in
   *  `capabilities/default.json`. */
  setWindowTheme: (theme: 'light' | 'dark' | null) => getCurrentWindow().setTheme(theme),
```

- [ ] **Step 6: Gate and commit**

```bash
cargo fmt --all && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo bench -p photon-core --bench grid --no-run
npm run check && npm test
git add crates/photon-app ui/src/lib/api.ts
git commit -m "feat(ipc): theme and set_theme, and the grant to theme the title bar

No test: both commands only delegate to Library::theme and set_theme, which
are tested in photon-core, and the handler registration and the capability
fail only inside a running webview. Both are on the smoke checklist.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: `tokens.css`, the aliases, and the contrast test

**Files:**
- Create: `ui/src/tokens.css`
- Modify: `ui/src/app.css`
- Modify: `ui/vite.config.ts`
- Create: `ui/src/lib/tokens.test.ts`

**Interfaces:**
- Produces: the CSS custom properties below, resolved on any element carrying `data-theme`, and on `:root` (light) when none is set. Later PRs use only these names. The old names `--bg --panel --panel-2 --text --muted --accent --danger` keep working until PR 5 deletes them.

- [ ] **Step 1: Let vitest read CSS**

In `ui/vite.config.ts`, change the `test` line to:

```ts
  // `css: true`: vitest otherwise turns every CSS import into an empty string, `?raw`
  // included, and tokens.test.ts reads tokens.css as text.
  test: { include: ['src/**/*.test.ts'], environment: 'node', css: true },
```

- [ ] **Step 2: Write the failing test**

Create `ui/src/lib/tokens.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import css from '../tokens.css?raw';

type Tokens = Record<string, string>;

/** The custom properties of every rule whose selector list contains `selector` exactly. */
function block(source: string, selector: string): Tokens {
  const out: Tokens = {};
  const bare = source.replace(/\/\*[\s\S]*?\*\//g, '');
  for (const rule of bare.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    const selectors = rule[1].split(',').map((s) => s.trim());
    if (!selectors.includes(selector)) continue;
    for (const decl of rule[2].matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) out[decl[1]] = decl[2].trim();
  }
  return out;
}

/** `#rrggbb` or `#rrggbbaa` to [r, g, b, alpha 0..1]. */
function rgba(hex: string): [number, number, number, number] {
  const m = /^#([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(hex);
  if (!m) throw new Error(`not a 6- or 8-digit hex colour: ${hex}`);
  const n = parseInt(m[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255, m[2] ? parseInt(m[2], 16) / 255 : 1];
}

/** A translucent colour as it appears over an opaque ground. */
function over(top: string, ground: string): string {
  const [r, g, b, a] = rgba(top);
  const [R, G, B] = rgba(ground);
  const mix = (f: number, bk: number) => Math.round(f * a + bk * (1 - a));
  return '#' + [mix(r, R), mix(g, G), mix(b, B)].map((v) => v.toString(16).padStart(2, '0')).join('');
}

/** WCAG 2.x relative luminance and contrast ratio. */
function luminance(hex: string): number {
  const [r, g, b] = rgba(hex).slice(0, 3).map((v) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const themes = {
  light: block(css, "[data-theme='light']"),
  dark: block(css, "[data-theme='dark']"),
};

describe('the contrast helpers', () => {
  it('agree with the WCAG reference values', () => {
    expect(contrast('#000000', '#ffffff')).toBeCloseTo(21, 5);
    expect(contrast('#777777', '#ffffff')).toBeCloseTo(4.48, 2);
    expect(over('#00000080', '#ffffff')).toBe('#7f7f7f');
  });
});

describe.each(Object.entries(themes))('the %s theme', (_name, t) => {
  const grounds = ['--surface', '--chrome'] as const;

  it.each(grounds)('has readable text on %s', (ground) => {
    expect(contrast(t['--text'], t[ground])).toBeGreaterThanOrEqual(4.5);
    expect(contrast(t['--text-dim'], t[ground])).toBeGreaterThanOrEqual(4.5);
    expect(contrast(t['--danger'], t[ground])).toBeGreaterThanOrEqual(4.5);
  });

  it.each(grounds)('has an accent that stands out on %s', (ground) => {
    expect(contrast(t['--accent'], t[ground])).toBeGreaterThanOrEqual(3);
  });

  it('has readable text on a primary button', () => {
    expect(contrast(t['--on-accent'], t['--accent'])).toBeGreaterThanOrEqual(4.5);
  });

  it('keeps a sidebar row readable when hovered and when active', () => {
    // A hovered row still shows its dim count; an active row's count switches to --text,
    // because --text-dim does not reach 4.5 over --accent-soft (spec §2, §5).
    expect(contrast(t['--text-dim'], over(t['--hover'], t['--chrome']))).toBeGreaterThanOrEqual(4.5);
    expect(contrast(t['--text'], over(t['--accent-soft'], t['--chrome']))).toBeGreaterThanOrEqual(4.5);
  });
});

describe('the two themes', () => {
  it('define the same tokens, bar the viewer-only glass', () => {
    const glass = ['--glass', '--glass-line'];
    const names = (t: Tokens) => Object.keys(t).filter((n) => !glass.includes(n)).sort();
    expect(names(themes.light).length).toBeGreaterThan(10);
    expect(names(themes.dark)).toEqual(names(themes.light));
    for (const name of glass) expect(themes.dark[name]).toBeDefined();
  });
});
```

- [ ] **Step 3: Run it to see it fail**

Run: `npm test -w ui -- src/lib/tokens.test.ts`
Expected: FAIL, cannot resolve `../tokens.css?raw`.

- [ ] **Step 4: Create `ui/src/tokens.css`**

```css
/* Every colour in photon, and the scales. Components use these names and no literal; the
   contrast between them is asserted by lib/tokens.test.ts, which reads this file as text -
   so keep each value a plain 6- or 8-digit hex, and keep braces out of comments. */

/* Light is also what an element with no data-theme resolves to, so a page that loses
   theme-boot.js still renders. */
:root,
[data-theme='light'] {
  color-scheme: light;
  --surface: #ffffff;
  --chrome: #ebebed;
  --raised: #ffffff;
  --field: #0000000d;
  --hover: #0000000a;
  --line: #00000018;
  --text: #1f1f23;
  --text-dim: #5f5f67;
  --accent: #1f6fd6;
  /* A literal, not color-mix(): the webview on Linux is whatever WebKitGTK the
     distribution ships. */
  --accent-soft: #1f6fd633;
  --on-accent: #ffffff;
  --danger: #c4302b;
  --star: #e0a100;
}

/* Must stay after the light block. On the root element both selectors have the same
   specificity, so source order is the only thing that lets data-theme="dark" beat :root. */
[data-theme='dark'] {
  color-scheme: dark;
  --surface: #1b1b1e;
  --chrome: #2c2c31;
  --raised: #36363c;
  --field: #ffffff12;
  --hover: #ffffff0d;
  --line: #ffffff14;
  --text: #ededf0;
  --text-dim: #a6a6af;
  --accent: #62a0ea;
  --accent-soft: #62a0ea33;
  --on-accent: #111111;
  --danger: #ff6b6b;
  --star: #ffd24a;
  /* The viewer is dark in both themes, so its glass exists only here. */
  --glass: #26262bd9;
  --glass-line: #ffffff1a;
}

/* The names components used before the tokens, removed in the reskin's last PR. Declared
   on [data-theme] as well as :root because a var() is resolved where it is declared: on
   :root alone, a dark subtree inside a light app would inherit the light values. */
:root,
[data-theme] {
  --bg: var(--surface);
  --panel: var(--chrome);
  --panel-2: var(--raised);
  --muted: var(--text-dim);
}

:root {
  --r-1: 4px;
  --r-2: 6px;
  --r-3: 8px;
  --r-4: 12px;
  --s-1: 4px;
  --s-2: 8px;
  --s-3: 12px;
  --s-4: 16px;
  --s-5: 24px;
  --s-6: 32px;
  --t-1: 11px;
  --t-2: 12px;
  --t-3: 13px;
  --t-4: 15px;
  --t-5: 18px;
  --shadow-menu: 0 6px 24px #00000040;
  --shadow-dialog: 0 16px 48px #00000059;
  accent-color: var(--accent);
}

:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
/* Containers focused from script - the viewer, the settings dialog, an open menu - so that
   keys reach them. A ring around the whole window says nothing. */
[tabindex='-1']:focus-visible {
  outline: none;
}
```

`--text`, `--accent` and `--danger` need no alias: the new tokens have the same names.

- [ ] **Step 5: Point `app.css` at it**

Replace the whole of `ui/src/app.css` with:

```css
@import './tokens.css';

:root {
  font-family: system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif;
  font-size: 14px;
  color: var(--text);
  background: var(--surface);
}
* { box-sizing: border-box; }
html, body, #app { margin: 0; height: 100%; overflow: hidden; }
button { font: inherit; color: inherit; }
```

- [ ] **Step 6: Run the tests**

Run: `npm test -w ui -- src/lib/tokens.test.ts`
Expected: all pass. If a contrast assertion fails, the rule wins: adjust the value in `tokens.css`, and the table in spec §2 to match.

- [ ] **Step 7: Prove the tests discriminate**

Apply each probe to `tokens.css`, run the test file, see the named test fail, undo.

1. Dark `--text-dim: #a6a6af` → `#9a9aa3`. Expected: dark "keeps a sidebar row readable" fails (about 4.2).
2. Light `--accent: #1f6fd6` → `#2f7de1`. Expected: light "has readable text on a primary button" fails (about 4.1).
3. Delete the dark `--star` line. Expected: "define the same tokens" fails.

- [ ] **Step 8: Gate and commit**

```bash
npm run check && npm test
git add ui/src/tokens.css ui/src/app.css ui/src/lib/tokens.test.ts ui/vite.config.ts
git commit -m "feat(ui): design tokens in light and dark, with the old names aliased

Components are untouched: their variables now resolve through tokens.css, so
they follow data-theme already, and their remaining literals are replaced one
surface at a time in the PRs that follow. tokens.test.ts holds the palette to
WCAG contrast and to light/dark parity; each assertion was run against a value
that breaks it.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: `createTheme`

**Files:**
- Create: `ui/src/lib/theme.svelte.ts`
- Create: `ui/src/lib/theme.svelte.test.ts`

**Interfaces:**
- Consumes: `type ThemeChoice` from `./api` (Task 2).
- Produces:
  - `type ResolvedTheme = 'light' | 'dark'`
  - `resolveTheme(choice: ThemeChoice, osDark: boolean): ResolvedTheme`
  - `interface ThemeDeps { load(): Promise<ThemeChoice>; save(choice: ThemeChoice): Promise<void>; media: { dark(): boolean; onchange(cb: () => void): () => void }; apply(resolved: ResolvedTheme, choice: ThemeChoice): void; onerror(e: unknown): void }`
  - `createTheme(deps: ThemeDeps)` returning `{ readonly choice: ThemeChoice; readonly resolved: ResolvedTheme; init(): Promise<void>; set(choice: ThemeChoice): Promise<void>; dispose(): void }`

- [ ] **Step 1: Write the failing tests**

Create `ui/src/lib/theme.svelte.test.ts`:

```ts
import { describe, expect, it, vi } from 'vitest';
import type { ThemeChoice } from './api';
import { createTheme, resolveTheme } from './theme.svelte';

function setup(opts: { stored?: ThemeChoice; osDark?: boolean; load?: () => Promise<ThemeChoice> } = {}) {
  let osDark = opts.osDark ?? false;
  const listeners = new Set<() => void>();
  const apply = vi.fn();
  const save = vi.fn(async (_choice: ThemeChoice) => {});
  const onerror = vi.fn();
  const theme = createTheme({
    load: opts.load ?? (async () => opts.stored ?? 'system'),
    save,
    media: {
      dark: () => osDark,
      onchange: (cb) => {
        listeners.add(cb);
        return () => listeners.delete(cb);
      },
    },
    apply,
    onerror,
  });
  const setOs = (dark: boolean) => {
    osDark = dark;
    for (const cb of [...listeners]) cb();
  };
  return { theme, apply, save, onerror, setOs, listeners };
}

describe('resolveTheme', () => {
  it('follows the desktop only when asked to', () => {
    expect(resolveTheme('system', true)).toBe('dark');
    expect(resolveTheme('system', false)).toBe('light');
    expect(resolveTheme('light', true)).toBe('light');
    expect(resolveTheme('dark', false)).toBe('dark');
  });
});

describe('createTheme', () => {
  it('applies the stored choice on init', async () => {
    const { theme, apply } = setup({ stored: 'dark', osDark: false });
    await theme.init();
    expect(theme.choice).toBe('dark');
    expect(theme.resolved).toBe('dark');
    expect(apply).toHaveBeenLastCalledWith('dark', 'dark');
  });

  it('follows the desktop while the choice is system', async () => {
    const { theme, apply, setOs } = setup({ stored: 'system', osDark: false });
    await theme.init();
    expect(apply).toHaveBeenLastCalledWith('light', 'system');
    setOs(true);
    expect(theme.resolved).toBe('dark');
    expect(apply).toHaveBeenLastCalledWith('dark', 'system');
  });

  it('ignores the desktop once the user has pinned a theme', async () => {
    const { theme, apply, setOs } = setup({ stored: 'light', osDark: false });
    await theme.init();
    apply.mockClear();
    setOs(true);
    expect(theme.resolved).toBe('light');
    expect(apply).not.toHaveBeenCalled();
  });

  it('applies a new choice before saving it', async () => {
    const { theme, apply, save } = setup({ stored: 'system', osDark: false });
    await theme.init();
    let appliedWhenSaved: unknown;
    save.mockImplementationOnce(async () => {
      appliedWhenSaved = apply.mock.lastCall;
    });
    await theme.set('dark');
    expect(appliedWhenSaved).toEqual(['dark', 'dark']);
    expect(save).toHaveBeenCalledWith('dark');
    expect(theme.choice).toBe('dark');
  });

  it('keeps the applied theme and reports when the save fails', async () => {
    const { theme, apply, save, onerror } = setup({ stored: 'system', osDark: false });
    await theme.init();
    const failure = new Error('disk full');
    save.mockRejectedValueOnce(failure);
    await theme.set('dark');
    expect(theme.resolved).toBe('dark');
    expect(apply).toHaveBeenLastCalledWith('dark', 'dark');
    expect(onerror).toHaveBeenCalledWith(failure);
  });

  it('falls back to the desktop and reports when the stored choice cannot be read', async () => {
    const failure = new Error('locked');
    const { theme, apply, onerror } = setup({ osDark: true, load: async () => Promise.reject(failure) });
    await theme.init();
    expect(theme.choice).toBe('system');
    expect(apply).toHaveBeenLastCalledWith('dark', 'system');
    expect(onerror).toHaveBeenCalledWith(failure);
  });

  it('stops listening to the desktop on dispose', async () => {
    const { theme, apply, setOs, listeners } = setup({ stored: 'system', osDark: false });
    await theme.init();
    theme.dispose();
    expect(listeners.size).toBe(0);
    apply.mockClear();
    setOs(true);
    expect(apply).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run them to see them fail**

Run: `npm test -w ui -- src/lib/theme.svelte.test.ts`
Expected: FAIL, cannot resolve `./theme.svelte`.

- [ ] **Step 3: Implement**

Create `ui/src/lib/theme.svelte.ts`:

```ts
import type { ThemeChoice } from './api';

export type ResolvedTheme = 'light' | 'dark';

export interface ThemeDeps {
  load(): Promise<ThemeChoice>;
  save(choice: ThemeChoice): Promise<void>;
  /** The desktop's colour scheme. `onchange` returns its own unsubscribe. */
  media: { dark(): boolean; onchange(cb: () => void): () => void };
  /** The side effects: the DOM attribute, the launch mirror, the title bar. */
  apply(resolved: ResolvedTheme, choice: ThemeChoice): void;
  onerror(e: unknown): void;
}

export function resolveTheme(choice: ThemeChoice, osDark: boolean): ResolvedTheme {
  if (choice === 'system') return osDark ? 'dark' : 'light';
  return choice;
}

/** The colour scheme: what the user chose, and what that comes to on this desktop right
 *  now. Everything that touches the DOM, storage or Tauri is injected, so this runs under
 *  vitest's node environment. */
export function createTheme(deps: ThemeDeps) {
  let choice = $state<ThemeChoice>('system');
  let resolved = $state<ResolvedTheme>(resolveTheme('system', deps.media.dark()));
  let unsubscribe: (() => void) | undefined;

  function refresh() {
    resolved = resolveTheme(choice, deps.media.dark());
    deps.apply(resolved, choice);
  }

  return {
    get choice() {
      return choice;
    },
    get resolved() {
      return resolved;
    },

    async init() {
      // A pinned theme is the user's answer to a desktop that reports the wrong scheme
      // (WebKitGTK does), so a change on the desktop must not undo it.
      unsubscribe = deps.media.onchange(() => {
        if (choice === 'system') refresh();
      });
      try {
        choice = await deps.load();
      } catch (e) {
        deps.onerror(e);
      }
      refresh();
    },

    /** Applies first and saves second, so the click is answered at once. A failed save
     *  keeps the theme for this session: reverting it would punish the user for a disk
     *  error with a flash. */
    async set(next: ThemeChoice) {
      choice = next;
      refresh();
      try {
        await deps.save(next);
      } catch (e) {
        deps.onerror(e);
      }
    },

    dispose() {
      unsubscribe?.();
      unsubscribe = undefined;
    },
  };
}

export type Theme = ReturnType<typeof createTheme>;
```

- [ ] **Step 4: Run the tests**

Run: `npm test -w ui -- src/lib/theme.svelte.test.ts`
Expected: all pass.

- [ ] **Step 5: Prove the tests discriminate**

Apply each probe to `theme.svelte.ts`, run the file, see the named test fail, undo.

1. In `resolveTheme`, return `'light'` for `system` regardless. Expected: `resolveTheme` test and "follows the desktop" fail.
2. In `init`, change `if (choice === 'system') refresh();` to `refresh();`. Expected: "ignores the desktop once the user has pinned a theme" fails on `apply` having been called. (`resolved` stays `light` either way, which is why the test asserts on `apply`.)
3. In `set`, move `refresh();` to after the `try`/`catch`. Expected: "applies a new choice before saving it" fails.
4. In `set`, remove the `try`/`catch`, leaving `await deps.save(next);`. Expected: "keeps the applied theme and reports" fails with the rejection.
5. In `init`, remove the `try`/`catch`, leaving `choice = await deps.load();`. Expected: "falls back to the desktop" fails with the rejection.
6. Empty the body of `dispose`. Expected: "stops listening" fails.
7. In `init`, delete `refresh();` after the `try`. Expected: "applies the stored choice on init" fails.

- [ ] **Step 6: Gate and commit**

```bash
npm run check && npm test
git add ui/src/lib/theme.svelte.ts ui/src/lib/theme.svelte.test.ts
git commit -m "feat(ui): createTheme resolves the choice against the desktop's scheme

Pure and dependency-injected, so it is tested in the node environment; the
DOM, storage and Tauri side effects are wired in a separate module. Seven
probes, one per behaviour, each failed the test that covers it.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: No flash at launch, and the production wiring

**Files:**
- Create: `ui/public/theme-boot.js`
- Create: `ui/src/lib/theme-boot.test.ts`
- Modify: `ui/index.html`
- Create: `ui/src/lib/app-theme.svelte.ts`
- Modify: `ui/src/App.svelte:53-56` (the `onMount`)

**Interfaces:**
- Consumes: `createTheme`, `ResolvedTheme` (Task 4); `api.theme`, `api.setTheme`, `api.setWindowTheme`, `ThemeChoice` (Task 2); `library.reportError` from `./library.svelte`.
- Produces: `export const THEME_MIRROR_KEY = 'photon.theme'` and `export const theme: Theme` from `ui/src/lib/app-theme.svelte.ts`. The boot script reads the same key; the string is repeated there because a classic script cannot import.

- [ ] **Step 1: Write the failing test**

Create `ui/src/lib/theme-boot.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import boot from '../../public/theme-boot.js?raw';
import { THEME_MIRROR_KEY } from './app-theme.svelte';

/** Runs the boot script against fakes of the three globals it touches. */
function run(opts: { stored?: string | null; osDark: boolean; storageThrows?: boolean }): string | undefined {
  const dataset: Record<string, string> = {};
  const localStorage = {
    getItem: (key: string) => {
      if (opts.storageThrows) throw new Error('blocked');
      return key === THEME_MIRROR_KEY ? (opts.stored ?? null) : null;
    },
  };
  const matchMedia = (query: string) => ({ matches: query === '(prefers-color-scheme: dark)' && opts.osDark });
  new Function('localStorage', 'matchMedia', 'document', boot)(localStorage, matchMedia, {
    documentElement: { dataset },
  });
  return dataset.theme;
}

describe('theme-boot.js', () => {
  it('uses a pinned theme whatever the desktop says', () => {
    expect(run({ stored: 'light', osDark: true })).toBe('light');
    expect(run({ stored: 'dark', osDark: false })).toBe('dark');
  });

  it('follows the desktop for system, for nothing stored, and for nonsense', () => {
    for (const stored of ['system', null, 'sepia']) {
      expect(run({ stored, osDark: true })).toBe('dark');
      expect(run({ stored, osDark: false })).toBe('light');
    }
  });

  it('follows the desktop when storage cannot be read', () => {
    expect(run({ storageThrows: true, osDark: true })).toBe('dark');
  });
});
```

- [ ] **Step 2: Run it to see it fail**

Run: `npm test -w ui -- src/lib/theme-boot.test.ts`
Expected: FAIL, cannot resolve `../../public/theme-boot.js?raw`.

- [ ] **Step 3: Create `ui/public/theme-boot.js`**

```js
// Sets the theme before the first paint. The user's choice lives in library.db, which
// answers only after the app has started, so a Light choice on a dark desktop would paint
// dark first; app-theme.svelte.ts mirrors the choice into localStorage for this script.
// A file rather than an inline script because the CSP has no 'unsafe-inline' for scripts,
// and a classic script rather than part of the bundle because a module is deferred.
(function () {
  var choice = null;
  try {
    choice = localStorage.getItem('photon.theme');
  } catch (e) {
    // Storage can be blocked or cleared; the desktop's scheme is the fallback.
  }
  var dark = choice === 'dark' || (choice !== 'light' && matchMedia('(prefers-color-scheme: dark)').matches);
  document.documentElement.dataset.theme = dark ? 'dark' : 'light';
})();
```

- [ ] **Step 4: Load it from `ui/index.html`**

In `<head>`, after the `<title>` line:

```html
    <!-- Classic and render-blocking on purpose: see the file. -->
    <script src="/theme-boot.js"></script>
```

- [ ] **Step 5: Create `ui/src/lib/app-theme.svelte.ts`**

```ts
import { api } from './api';
import { library } from './library.svelte';
import { createTheme } from './theme.svelte';

/** Read by `public/theme-boot.js` before the first paint. The boot script repeats this
 *  string, since a classic script cannot import; theme-boot.test.ts reads with this one. */
export const THEME_MIRROR_KEY = 'photon.theme';

const osDark = () => window.matchMedia('(prefers-color-scheme: dark)');

/** The app's theme. The logic is `createTheme`'s; this is the wiring it is injected with,
 *  in its own module so importing `createTheme` in a test touches no `window`. */
export const theme = createTheme({
  load: () => api.theme(),
  save: (choice) => api.setTheme(choice),
  media: {
    // `createTheme` asks once at creation, which is at import - and theme-boot.test.ts
    // imports this module for the key, under node, where there is no window.
    dark: () => typeof window !== 'undefined' && osDark().matches,
    onchange: (cb) => {
      const query = osDark();
      query.addEventListener('change', cb);
      return () => query.removeEventListener('change', cb);
    },
  },
  apply: (resolved, choice) => {
    document.documentElement.dataset.theme = resolved;
    try {
      localStorage.setItem(THEME_MIRROR_KEY, choice);
    } catch {
      // Only the next launch's first frame depends on it.
    }
    // null hands the title bar back to the desktop.
    api.setWindowTheme(choice === 'system' ? null : choice).catch(library.reportError);
  },
  onerror: library.reportError,
});
```

Everything else that touches `window`, `document` or `localStorage` is inside a function
that only runs in the app, so the `typeof window` guard is the only concession to the test's
import.

- [ ] **Step 6: Wire it into `App.svelte`**

Add the import beside the others:

```ts
  import { theme } from './lib/app-theme.svelte';
```

Replace the `onMount` with:

```ts
  onMount(() => {
    library.init().catch(library.reportError);
    // `init` reports its own failures; theme-boot.js has already set the first frame.
    void theme.init();
    return () => {
      library.dispose();
      theme.dispose();
    };
  });
```

- [ ] **Step 7: Run the tests**

Run: `npm test -w ui -- src/lib/theme-boot.test.ts`
Expected: all pass.

- [ ] **Step 8: Prove the tests discriminate**

Apply each probe to `theme-boot.js`, run the file, see the named test fail, undo.

1. Change `choice !== 'light' &&` to `choice === 'system' &&`. Expected: "follows the desktop for system, for nothing stored, and for nonsense" fails on `null`.
2. Remove the `try`/`catch`, leaving the `getItem` line. Expected: "follows the desktop when storage cannot be read" fails with `blocked`.
3. Change `'photon.theme'` to `'theme'`. Expected: "uses a pinned theme" fails — this is the test that holds the two copies of the key together.

- [ ] **Step 9: Confirm the build ships the boot script**

```bash
npm run build -w ui && ls ui/dist/theme-boot.js && grep -c 'src="/theme-boot.js"' ui/dist/index.html
```

Expected: the file is listed and the count is `1`. `ui/dist` is already ignored; check `git status --short` shows nothing from it.

- [ ] **Step 10: Gate and commit**

```bash
npm run check && npm test
git add ui/public/theme-boot.js ui/index.html ui/src/lib/theme-boot.test.ts ui/src/lib/app-theme.svelte.ts ui/src/App.svelte
git commit -m "feat(ui): the theme is applied before first paint and follows the setting

theme-boot.js is a file because the CSP forbids inline scripts, and classic
because a module is deferred. It is tested by running its source against fake
globals; three probes each failed their test. app-theme.svelte.ts and the
onMount line are DOM and Tauri wiring with no seam under vitest's node
environment, so they are covered by the smoke checklist instead.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: The Appearance section in Settings

**Files:**
- Modify: `ui/src/lib/settings.ts:3`
- Modify: `ui/src/components/Settings.svelte` (imports; the `<nav>` at ~line 224; the section chain at ~line 331; the `<style>` block at ~line 416)

**Interfaces:**
- Consumes: `theme` from `../lib/app-theme.svelte` (`theme.choice`, `theme.set(choice)`); `ThemeChoice` from `../lib/api`.
- Produces: `SettingsSection` now includes `'appearance'`.

- [ ] **Step 1: Extend the section type**

In `ui/src/lib/settings.ts`:

```ts
export type SettingsSection = 'folders' | 'appearance' | 'tags' | 'slideshow' | 'about';
```

- [ ] **Step 2: The nav entry**

In `Settings.svelte`, add `type ThemeChoice` to the `../lib/api` import, and add:

```ts
  import { theme } from '../lib/app-theme.svelte';

  const THEMES: { value: ThemeChoice; label: string }[] = [
    { value: 'system', label: 'System' },
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
  ];
```

In the `<nav>`, between the Folders and Tags buttons:

```svelte
        <button class:active={current === 'appearance'} aria-current={current === 'appearance'} onclick={() => (current = 'appearance')}>
          Appearance
        </button>
```

- [ ] **Step 3: The section**

In the `{#if current === 'folders'} … {:else if …}` chain, immediately before `{:else if current === 'slideshow'}`:

```svelte
        {:else if current === 'appearance'}
          <h2>Appearance</h2>
          <p class="hint">System follows your desktop. The photo viewer is always dark, so every photo is seen against the same ground.</p>
          <div class="segmented" role="radiogroup" aria-label="Theme">
            {#each THEMES as option (option.value)}
              <button
                role="radio"
                aria-checked={theme.choice === option.value}
                class:checked={theme.choice === option.value}
                onclick={() => theme.set(option.value)}
              >
                {option.label}
              </button>
            {/each}
          </div>
```

- [ ] **Step 4: Its styles, in tokens only**

In the `<style>` block, after the `.interval input` rule:

```css
  .segmented { display: inline-flex; gap: 2px; padding: 2px; border-radius: var(--r-3); background: var(--field); }
  .segmented button { padding: 4px 14px; border: 0; border-radius: var(--r-2); background: none; cursor: pointer; }
  .segmented button:hover { background: var(--hover); }
  .segmented button.checked { background: var(--accent); color: var(--on-accent); }
```

- [ ] **Step 5: Gate**

Run: `npm run check && npm test`
Expected: 0 errors, 0 warnings; all tests pass. `svelte-check` is what verifies this task: a `SettingsSection` literal that the new member breaks, or an a11y warning on the radio buttons, fails it.

- [ ] **Step 6: Commit**

```bash
git add ui/src/lib/settings.ts ui/src/components/Settings.svelte
git commit -m "feat(settings): an Appearance section with System, Light and Dark

No test: the section is markup bound to theme.choice and theme.set, both
tested in theme.svelte.test.ts, and there is no component harness. On the
smoke checklist.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: The icon set

**Files:**
- Create: `ui/src/lib/icons.ts`
- Create: `ui/src/components/Icon.svelte`
- Create: `THIRD-PARTY-NOTICES.md`

**Interfaces:**
- Produces: `export type IconName` (the union of the eighteen keys below); `export const ICONS: Record<IconName, string>`; `Icon.svelte` with props `{ name: IconName; size?: number; filled?: boolean }`. Nothing uses it in this PR; PRs 2–5 replace the glyphs with it.

- [ ] **Step 1: Create `ui/src/lib/icons.ts`**

```ts
// Icons from Lucide (https://lucide.dev), lucide-static 1.47.0. ISC License, Copyright (c)
// Lucide Icons and Contributors; chevron-down, chevron-right, clock, info, search and x
// derive from Feather, MIT License, Copyright (c) 2013-present Cole Bemis. Both licences
// are reproduced in THIRD-PARTY-NOTICES.md at the repository root.
//
// Each value is the inside of the icon's <svg>, on Lucide's 24-unit grid. Vendored rather
// than depended on: eighteen icons do not justify a package and its update churn.

export type IconName =
  | 'chevron-down'
  | 'chevron-right'
  | 'clock'
  | 'copy'
  | 'crop'
  | 'folder'
  | 'info'
  | 'pause'
  | 'play'
  | 'rotate-ccw'
  | 'rotate-cw'
  | 'search'
  | 'settings'
  | 'star'
  | 'tag'
  | 'triangle-alert'
  | 'user'
  | 'x';

export const ICONS: Record<IconName, string> = {
  'chevron-down': '<path d="m6 9 6 6 6-6"/>',
  'chevron-right': '<path d="m9 18 6-6-6-6"/>',
  clock: '<circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/>',
  copy: '<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>',
  crop: '<path d="M6 2v14a2 2 0 0 0 2 2h14"/><path d="M18 22V8a2 2 0 0 0-2-2H2"/>',
  folder:
    '<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>',
  info: '<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>',
  pause: '<rect x="14" y="3" width="5" height="18" rx="1"/><rect x="5" y="3" width="5" height="18" rx="1"/>',
  play: '<path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"/>',
  'rotate-ccw': '<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>',
  'rotate-cw': '<path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/>',
  search: '<path d="m21 21-4.34-4.34"/><circle cx="11" cy="11" r="8"/>',
  settings:
    '<path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915"/><circle cx="12" cy="12" r="3"/>',
  star: '<path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z"/>',
  tag: '<path d="M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.42z"/><circle cx="7.5" cy="7.5" r=".5" fill="currentColor"/>',
  'triangle-alert':
    '<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>',
  user: '<path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/>',
  x: '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
};
```

Verify the path data against the source rather than trusting this plan: in a scratch directory outside the repo, `npm pack lucide-static@1.47.0 && tar xzf lucide-static-1.47.0.tgz`, then compare each `package/icons/<name>.svg` body with its entry.

- [ ] **Step 2: Create `ui/src/components/Icon.svelte`**

```svelte
<script lang="ts">
  import { ICONS, type IconName } from '../lib/icons';

  let { name, size = 16, filled = false }: { name: IconName; size?: number; filled?: boolean } = $props();
</script>

<!-- Decoration: the button around it carries the aria-label. `{@html}` is safe here
     because ICONS is a compile-time constant, never user data. -->
<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill={filled ? 'currentColor' : 'none'}
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {@html ICONS[name]}
</svg>

<style>
  /* An inline SVG sits on the text baseline and drags its line taller; as a block-level
     flex item it takes exactly its own box. */
  svg { display: block; flex: none; }
</style>
```

- [ ] **Step 3: Create `THIRD-PARTY-NOTICES.md`**

From the scratch directory of Step 1, copy `package/LICENSE` verbatim under this header (the file holds the ISC text, the list of Feather-derived icons, and the MIT text; all three parts are required):

```markdown
# Third-party notices

photon's own licence is in `LICENSE`. This file reproduces the licences of third-party
material compiled into it.

## Lucide icons

The icons in `ui/src/lib/icons.ts` are from Lucide (https://lucide.dev), lucide-static
1.47.0, unmodified. Its licence file follows in full.
```

Then the verbatim contents of `package/LICENSE` inside a fenced `text` block.

- [ ] **Step 4: Gate**

```bash
npm run check && npm test
cargo run -p xtask -- metadata
```

Expected: 0 errors, 0 warnings, all tests pass, and `metadata` still reports complete. If `svelte-check` warns about `{@html}`, do not suppress it blindly: read the warning, and if it is the generic XSS advisory, add `<!-- svelte-ignore -->` with that exact code directly above the `{@html}` line, keeping the comment that explains why it is safe.

- [ ] **Step 5: Commit**

```bash
git add ui/src/lib/icons.ts ui/src/components/Icon.svelte THIRD-PARTY-NOTICES.md
git commit -m "feat(ui): eighteen Lucide icons behind one Icon component

Vendored path data, no dependency. Nothing renders one yet: each glyph is
replaced with the surface it belongs to, in the PRs that follow. No test:
IconName is a union, so a wrong name fails svelte-check, and the component is
markup with no component harness to render it.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: README, full gate, PR

**Files:**
- Modify: `README.md` (the `## Manual smoke checklist` section, and the Linux notes)

- [ ] **Step 1: Smoke checklist entries**

Read the existing checklist to match its voice, then add a group:

```markdown
### Theme

- [ ] Settings → Appearance → Dark, then Light, then System: the whole window follows at
      once, and so does the title bar. With System, switching the desktop between light
      and dark switches photon without a restart.
- [ ] Pin Light on a dark desktop (or Dark on a light one), quit, relaunch: the first frame
      is already the pinned theme, with no flash of the other.
- [ ] Pin a theme, then change the desktop's scheme: photon does not move.
- [ ] Checkboxes, the crop ratio menu and the scrollbars match the theme.
- [ ] Tab through the top bar, sidebar and Settings: every control shows the same blue
      focus ring, and opening the viewer or Settings draws no ring around the window.
```

- [ ] **Step 2: The Linux note**

Find the README's per-OS or Linux section (`grep -n -i "linux" README.md`) and add:

```markdown
**Theme on Linux.** photon follows the desktop's light or dark setting, but the system
webview (WebKitGTK) does not report it on every desktop. If photon stays light on a dark
desktop, pin it in Settings → Appearance.
```

- [ ] **Step 3: The whole gate, from clean**

```bash
cargo fmt --all && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo bench -p photon-core --bench grid --no-run
npm run check && npm test
cargo run -p xtask -- versions && cargo run -p xtask -- metadata
git status --short
```

Expected: everything passes; `git status` shows only `README.md`.

- [ ] **Step 4: Commit, push, open the PR**

```bash
git add README.md
git commit -m "docs(readme): smoke checks for the theme, and the WebKitGTK caveat

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
git push -u origin feat/ui-reskin-foundations
gh pr create --title "feat(ui): design tokens, light and dark themes, vendored icons (reskin 1/5)" --body "$(cat <<'EOF'
First of five PRs implementing `docs/superpowers/specs/2026-09-19-photon-ui-reskin-design.md`.

- `tokens.css`: semantic colour tokens in light and dark plus radius, spacing, type and shadow scales. The eight old variable names are aliased onto them, so no component changes here and every component already follows the theme.
- A System / Light / Dark setting: a row in the existing settings table (no schema change), `theme` / `set_theme` over IPC, `createTheme` for the logic, Settings → Appearance for the control, the native title bar following.
- `theme-boot.js` sets the theme before first paint. A file rather than an inline script because the CSP forbids inline scripts.
- `Icon.svelte` and eighteen vendored Lucide icons, unused until the surfaces are restyled.

**Light mode is rough after this PR on purpose**: components still hold dark-only colour literals, replaced one surface at a time in PRs 2–5. No release until the fifth has merged.

Tests: the palette is held to WCAG contrast and light/dark parity (`tokens.test.ts`), the theme logic and the boot script are tested with injected fakes, and every new test was run against a probe that breaks the line it covers. Not testable here, and on the README smoke checklist instead: the title bar, the no-flash launch, the Appearance section's markup.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 5: Wait for CI**

`gh pr checks <N> --watch`. Straight after a push it can answer "no checks reported" and exit 0; wait until checks exist before trusting it. Do not merge: the PR is for the user to review and smoke-test.
