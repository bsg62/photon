# photon — UI Reskin Design

**Date:** 2026-09-19
**Status:** Approved design, not yet implemented
**Builds on:** v0.15.0

## 1. What changes

photon's UI is dark-only, styled by eight CSS variables and by hex and `rgba()` literals
scattered through all ten components, with Unicode glyphs (`⚙ ★ 🕘 ✂ …`) for icons that each
OS draws in a different font.

This design is a **visual reskin**: a new design language on a real token system, light and
dark themes, and vendored SVG icons. Layout, DOM order, interactions, keyboard shortcuts and
copy stay as they are.

Decisions taken with mockups (kept under `.superpowers/brainstorm/`, untracked):

- **Feel:** native-modern — reads as a current desktop app on GNOME, macOS and Windows.
- **Surfaces:** "flat panels". Tinted chrome (top bar, sidebar, status bar), a plain content
  surface, hairline dividers, no shadows on anything that does not float.
- **Accent:** blue. It is the only colour photon itself brings; stars keep their own amber.
- **Theme:** follows the OS, with a System / Light / Dark override in Settings.
- **Viewer:** always dark, in both themes, so a photo is judged against the same ground. Its
  controls become one floating "glass" toolbar and a glass info card, in today's positions.
- **Icons:** Lucide (ISC), vendored as inline path data. No runtime dependency.
- **Font:** the system stack stays. It gains a type scale.

### Out of scope

Layout changes, new features, a custom title bar, a user-chosen accent, animation beyond
hover transitions, and any change to `ui/src/lib/` logic other than the two new modules
(`theme.svelte.ts`, `icons.ts`) and the `api.ts` mirror.

**The virtualisation constants do not change.** `TILE`, `GAP` and the header height in
`layout.ts` fix every row's offset; changing one shifts every scroll position and every
timeline mark. The reskin restyles inside those boxes.

## 2. Tokens

A new `ui/src/tokens.css`, imported by `app.css`. Components use semantic tokens only; a
component `<style>` block contains no colour literal (enforced, §7).

| Token | Light | Dark | Used for |
|---|---|---|---|
| `--surface` | `#ffffff` | `#1b1b1e` | grid area, dialog content |
| `--chrome` | `#ebebed` | `#2c2c31` | top bar, sidebar, status bar, settings nav |
| `--raised` | `#ffffff` | `#36363c` | menus, toasts, the timeline bubble |
| `--field` | `#0000000d` | `#ffffff12` | inputs, ghost buttons, chips |
| `--hover` | `#0000000a` | `#ffffff0d` | row and button hover |
| `--line` | `#00000018` | `#ffffff14` | every hairline divider |
| `--text` | `#1f1f23` | `#ededf0` | |
| `--text-dim` | `#5f5f67` | `#a6a6af` | counts, paths, captions, group headers |
| `--accent` | `#1f6fd6` | `#62a0ea` | |
| `--accent-soft` | `#1f6fd633` | `#62a0ea33` | the active row |
| `--on-accent` | `#ffffff` | `#111111` | text on a primary button |
| `--danger` | `#c4302b` | `#ff6b6b` | |
| `--star` | `#e0a100` | `#ffd24a` | |
| `--glass` | — | `#26262bd9` | viewer toolbar and info card |
| `--glass-line` | — | `#ffffff1a` | their 1px edge |

`--accent-soft` is a literal, not `color-mix()`: the webview on Linux is whatever WebKitGTK
the distribution ships, and a literal needs no minimum version.

Scales, identical in both themes:

- Radius `--r-1..4`: 4, 6, 8, 12 px.
- Spacing `--s-1..6`: 4, 8, 12, 16, 24, 32 px.
- Type `--t-1..5`: 11, 12, 13, 15, 18 px. The base is 13 px (was 14).
- Shadow `--shadow-menu`, `--shadow-dialog`. Only floating things cast one.

**Selectors.** The light block is `:root, [data-theme='light']`, the dark block
`[data-theme='dark']`, and `color-scheme` is set with each so native checkboxes, the crop
`<select>`, scrollbars and `<progress>` follow. The dark block comes second in the file: the
two selectors have equal specificity on `<html>`, so order is what lets `data-theme="dark"`
beat `:root`. `accent-color: var(--accent)` is global.
Because the blocks match any element and not only `:root`, the viewer's root element carries
`data-theme="dark"` and its whole subtree resolves dark tokens whatever the app theme is.
`--glass` and `--glass-line` exist only in the dark block; they are used only there.

**Glass.** `background: var(--glass)` at 85% opacity is the design. `backdrop-filter:
blur(18px)` is layered on as an enhancement, since it can be slow or absent on some Linux
GPUs; the toolbar must read correctly without it.

**Contrast rule.** `--text` and `--text-dim` reach 4.5:1 on both `--surface` and `--chrome`;
`--accent` reaches 3:1 on both; `--on-accent` reaches 4.5:1 on `--accent`; `--text-dim`
still reaches 4.5:1 on a hovered chrome row (`--hover` composited over `--chrome`), and
`--text` on an active one (`--accent-soft` over `--chrome`); in each theme. `--text-dim` does
*not* reach it on an active row (4.1:1), which is why an active row's count switches to
`--text` (§5).
Enforced by a test (§7). If a value in the table fails the test, the value is adjusted and
this table updated; the rule wins over the table. (Two values already differ from the
mockups for this reason: the light accent, since white on `#2f7de1` reached only 4.1:1, and
the dark `--text-dim`, since `#9a9aa3` reached only 4.2:1 on a hovered row.)

## 3. Theming

### Core and IPC

- `settings.rs`: a `theme` key in the existing key/value settings table — no schema change.
  `Library::theme()` returns a `ThemeChoice` (`System`, `Light`, `Dark`); never set, or any
  unrecognised stored value, reads as `System`, in the same defensive spirit as the slideshow
  interval's clamp. `Library::set_theme(choice)` stores it.
- `commands.rs` / `ipc.rs` / `app.rs`: `theme` and `set_theme`. `ThemeChoice` serialises
  lowercase.
- `api.ts`: `type ThemeChoice = 'system' | 'light' | 'dark'`, `api.theme()`,
  `api.setTheme(choice)`. Same commit as the Rust side, since nothing validates the mirror.
- `capabilities/default.json`: `core:window:allow-set-theme`.

### `ui/src/lib/theme.svelte.ts`

`createTheme({ load, save, media, apply })`, every dependency injected so it runs under
vitest's node environment:

- `load(): Promise<ThemeChoice>` and `save(choice): Promise<void>` — the two API calls.
- `media` — `{ dark: boolean; onchange(cb): () => void }`, wrapping
  `matchMedia('(prefers-color-scheme: dark)')`.
- `apply(resolved: 'light' | 'dark', choice: ThemeChoice)` — the side effects.

It exposes reactive `choice` and `resolved`, `init()`, `set(choice)` and `dispose()`.
`resolved` is `choice` unless that is `system`, when it is the media query's answer. A media
change re-applies only while `choice` is `system`. `set()` applies first and saves second, so
the UI responds at once; a failed save keeps the applied theme for the session and reports
through `library.reportError`.

The production `apply`:

1. sets `document.documentElement.dataset.theme = resolved`;
2. mirrors `choice` to `localStorage['photon.theme']`, in a try/catch;
3. calls the Tauri window's `setTheme(choice === 'system' ? null : choice)` so the native
   title bar follows.

### No flash at launch

The database read is asynchronous, so a Light override on a dark desktop would paint dark
first. A classic, render-blocking `<script src="/theme-boot.js">` in `index.html`'s `<head>`
(the file lives in `ui/public/`) reads `localStorage['photon.theme']` and sets `data-theme`
synchronously; a missing or unreadable mirror falls back to the media query. It is a file
rather than an inline script because the app's CSP is `default-src 'self'` with no
`'unsafe-inline'` for scripts, and loosening the CSP for five lines is the wrong trade. It
is a classic script rather than part of the bundle because a module script is deferred. When `load()` answers, the database wins if the two
differ (a library copied from another machine), at the cost of a one-frame correction.

### Settings

`SettingsSection` gains `'appearance'`, listed after Folders. The section holds one
three-way segmented control: System, Light, Dark.

### Known platform limit

WebKitGTK does not always report the desktop's colour scheme, so `System` can resolve to
light on a dark Linux desktop. The override is the answer; the README's Linux notes say so.

## 4. Icons

- `ui/src/lib/icons.ts`: `export const ICONS: Record<IconName, string>` of Lucide path data
  on its 24-unit grid. `IconName` is a union, so a misspelt name fails `svelte-check`. The
  file's header carries Lucide's ISC notice, and a root `THIRD-PARTY-NOTICES.md` records it
  for the installers' sake.
- `ui/src/components/Icon.svelte`: props `name`, `size` (default 16), `filled` (default
  false). One `aria-hidden` `<svg>`, `stroke="currentColor"`, stroke width 2, round caps and
  joins. Buttons keep their existing `aria-label` and `title`; the icon is decoration.

| Today | Icon | Where |
|---|---|---|
| ⚙ | `settings` | top bar |
| ★ ☆ | `star`, filled or not | sidebar, tile badge, viewer |
| 🕘 | `clock` | sidebar Recent |
| ⧉ | `copy` | sidebar Duplicates |
| ▸ ▾ | `chevron-right`, `chevron-down` | sidebar groups |
| ⚠ | `triangle-alert` | a broken tile |
| ✕ × | `x` | viewer close, settings close, toasts, chip remove |
| ↺ ↻ | `rotate-ccw`, `rotate-cw` | viewer |
| ✂ | `crop` | viewer |
| ▶ ⏸ | `play`, `pause` | slideshow |
| ⓘ | `info` | viewer |

New: `search` inside the search field; `folder`, `user`, `tag` beside the Albums, People and
Tags group headers.

## 5. Component language

- **Surfaces.** Chrome on `--chrome`, the grid on `--surface`, divided by 1px `--line`.
- **Rows** (sidebar items, menu items, settings nav): 28 px high, `--r-3`, inset 6 px from
  the panel edge. Hover `--hover`; active `--accent-soft` with normal-weight text, its count
in `--text` rather than `--text-dim` (contrast, §2). Counts
  lose their parentheses and become right-aligned `--text-dim`, `--t-1`, `tabular-nums`.
  Year headings and group headers are `--t-1`, weight 600, `--text-dim`.
- **Buttons.** Primary (`--accent` / `--on-accent`), ghost (`--field`), icon-only (30 px
  square, transparent until hover). All `--r-3`. Danger is ghost with `--danger` text.
- **Focus.** One global `:focus-visible { outline: 2px solid var(--accent); outline-offset:
  2px }` replaces the per-component treatments. The splitter keeps its accent fill.
- **Fields.** `--field`, no border at rest, the accent ring on focus. The search field gains
  a leading `search` icon.
- **Tiles.** `--r-2`. Selection is a 2px accent outline offset 2px, visible over any photo
  in either theme. Star badge in `--star` with a soft drop shadow. Offline dimming unchanged.
- **Folder headers.** Name `--t-4` weight 600, path `--t-1` `--text-dim`, in the existing
  header height.
- **Floating things** (context menus, toasts, timeline bubble): `--raised`, `--r-3`,
  `--shadow-menu`, rows as above.
- **Settings dialog.** Content on `--surface`, the section list on `--chrome` with sidebar
  rows, `--r-4`, `--shadow-dialog`.
- **Viewer.** Root carries `data-theme="dark"`; background stays `#000` (the one allowed
  literal). The bottom bar becomes a single glass toolbar, `--r-4`, grouped with separators:
  star | rotate left, rotate right, crop, Original | slideshow, info | caption. The info
  panel is a glass card in its current position; chips on `--field`, links in `--accent`.
  The crop bar is the same glass with Apply as the primary button. Face boxes and the crop
  overlay keep their geometry and take token colours. Slideshow `quiet` behaviour unchanged.
- **Motion.** `120ms ease-out` on hover and active backgrounds only, disabled under
  `prefers-reduced-motion`. The slideshow crossfade is untouched.

## 6. Delivery

Five PRs, each off `main`, each passing both gates. **No release is cut between PR 1 and
PR 5**: after PR 1 the app follows the OS theme while components still hold dark-only
literals, so light mode is rough until each surface lands.

1. **Foundations.** `tokens.css`; the eight old variable names defined as aliases of tokens
   (`--bg`→`--surface`, `--panel`→`--chrome`, `--panel-2`→`--raised`, `--text`,
   `--muted`→`--text-dim`, `--accent`, `--danger`); the global focus rule; the `theme`
   setting through core, IPC, capability and `api.ts`; `theme.svelte.ts`; the no-flash
   script; the Appearance section; `Icon.svelte`, `icons.ts`, the notices file; the contrast
   test.
2. **Shell.** `App.svelte`, `SearchBar`, `FolderTree`, `StatusBar`.
3. **Grid.** `Grid`, `Tile`, `Timeline`, `Toasts`.
4. **Viewer.** `Viewer.svelte`: CSS, plus markup only for icons and toolbar separators.
5. **Settings and cleanup.** `Settings.svelte`; delete the aliases; add the tripwire test;
   README smoke checklist and Linux note; CLAUDE.md.

After PR 5, one independent read of the whole result, pointed at what a restyle can change
in behaviour: `pointer-events` on the glass, `z-index` between the viewer's chrome and its
menus, `inert` regions, and the viewer's effect wiring around the moved toolbar markup.
Nothing here touches `pictureChanged` or the effects that feed it.

## 7. Testing

Every new test is shown to fail with its change reverted, per the repo's convention.

- **`theme.svelte.test.ts`** (fakes for all four dependencies): each choice resolves against
  each OS scheme; a media change re-applies only under `system`; `set()` applies then saves;
  a failed save keeps the applied theme and reports; `dispose()` unsubscribes.
- **`settings.rs`**: the theme defaults to `System`, round-trips, and an unrecognised stored
  value reads as `System`.
- **`theme-boot.test.ts`** (PR 1): imports `public/theme-boot.js` as raw text and runs it
  with fake `localStorage`, `matchMedia` and `document`: each stored choice, no stored
  choice under each OS scheme, and a `localStorage` that throws.
- **`tokens.test.ts`** (PR 1): reads `tokens.css` as raw text (vitest needs `test.css: true`
  for a CSS `?raw` import to be anything but empty), resolves the light and dark
  blocks, composites translucent values over their ground, and asserts the contrast rule of
  §2 with the WCAG formula. Pure string and arithmetic work, so it runs in the node
  environment. It also asserts both blocks define the same token names (bar the two glass
  tokens), so a token added to one theme and forgotten in the other fails.
- **`no-literals.test.ts`** (PR 5): reads every `ui/src/**/*.svelte` and `app.css`, and fails
  on a hex or `rgb(`/`rgba(` literal inside a `<style>` block (allow-list: the viewer's
  `#000`), and on any glyph from §4's table anywhere in a component. This is the automated
  definition of "the overhaul is finished" and what stops it eroding.
- **Not testable** (no component harness, and the GUI is never launched to verify): the look
  itself, `backdrop-filter`, the title bar following the theme. These go on the README's
  manual smoke checklist: each theme choice under each OS scheme; relaunch under a Light
  override on a dark desktop with no flash; the viewer's chrome dark in light mode; glass
  over a white and over a black photo; the focus ring on every control by keyboard; layout
  unchanged (the headless-Chromium layout probe can confirm row heights did not move).
