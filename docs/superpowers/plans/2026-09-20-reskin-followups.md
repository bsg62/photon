# Plan: the reskin's open follow-ups

Four items left open by the UI reskin (PRs #46-#52), recorded at the time and picked up now.
One item from the same list - `refresh_grid` holding the view lock across a rebuild - is
already closed: `refresh_grid` snapshots and builds unlocked, guarded by `epoch`/`seq`.

## 1. A failed theme *load* wipes the no-flash mirror

`createTheme.init()` catches a failing `deps.load()`, leaves `choice` at its default
`system`, and then calls `refresh()` unconditionally - which calls `apply(resolved, 'system')`
and so writes `system` over the `localStorage` mirror `theme-boot.js` reads before the first
paint. One failed load therefore costs a pinned user the no-flash boot *for every later
launch*, and hands the title bar back to the desktop.

Fix: a `mirrored()` dep. On a load failure the mirror - what the boot script already painted
with, and the last choice known to have been saved - becomes the choice, instead of `system`.
The screen then keeps showing what it painted, and `apply` rewrites the mirror with the same
value it held.

Test (`theme.test.ts`): a failing `load` with a `dark` mirror keeps `choice === 'dark'` and
applies `dark`; with no mirror it still falls back to `system`.

## 2. The grid viewport has no focus ring

`.viewport` is `tabindex="0"` with `outline: none`, so tabbing into the grid with nothing
selected shows nothing at all. The `outline: none` is not the global `[tabindex='-1']` rule -
this container is deliberately in the tab order.

Fix: `.viewport:focus-visible` gets the accent ring, drawn *inside* (`outline-offset: -2px`)
for the same reason the tile's selection ring is: the grid scrolls a row flush to the top of
the container, and anything outside the box is clipped.

Verified by `svelte-check` and the smoke checklist; there is no component harness.

## 3. The viewer toolbar runs under the zoom control at 800px

`.bar` is centred and `.zoom` is pinned bottom-right, so a wide enough bar reaches under it.
The caption's `max-width: 34vw` bounds the overlap without removing it.

Fix: reserve the zoom control's side on both sides of the centred bar with a `max-width`, and
let the caption shrink into what is left (`flex` + `min-width: 0`) rather than cap it at a
fraction of the viewport. Measured before and after with the headless-Chromium layout probe
(a static page holding the component's CSS, `getBoundingClientRect()` into `document.title`),
since vitest runs under node and cannot lay anything out.

## 4. Two colours under 4.5:1 on the viewer's glass

Measured with `tokens.test.ts`'s own helpers, over the brightest photo (white):

- `.info-link` is `--accent` on glass: **4.10**.
- `.tag-input`'s placeholder is `--text-dim` over `--field` over glass: **3.71**. `--field`
  is a white film, so it *lightens* the ground under light text.

Fix: a viewer-only `--accent-glass` (`#77aef0`, 4.82) beside `--glass`, used by `.info-link`;
and the tag input drops the `--field` film for the bare glass with a `--glass-line` hairline,
which brings the placeholder to 4.61. Both become assertions in `tokens.test.ts`, and
`--accent-glass` joins the glass-only names the light/dark parity test excludes.
