# photon — Slideshow Design

**Date:** 2026-09-18
**Status:** Approved design, implemented
**Builds on:** v0.14.2

## 1. Behaviour

A slideshow is a mode of the viewer, not a second viewer. `S` or the ▶ button plays the
current view from the photo on screen: fullscreen, crossfading, looping. Space pauses, the
arrows and the wheel step, Escape ends the show and stays in the viewer (a second Escape closes
it). The interval, default 4 s and clamped to 1–60 s, lives in the `settings` table
(`slideshow_interval_s`) and is edited under Settings → Slideshow. Starting a slideshow from
the grid was left out; the viewer is one keypress away.

## 2. The countdown belongs to a photo on screen

`createSlideshow` (`ui/src/lib/slideshow.svelte.ts`) holds the state machine so fake timers can
test it. The viewer reports `changed()` when the loader starts on a new photo and `shown()`
when the full image has decoded or the photo has failed for good. The countdown starts on
`shown()` and dies on `changed()`. A plain interval would count a slow decode against the
photo and flash past, or skip, a large file on a slow drive. A failed photo counts as shown,
so one bad file does not end the show. A view of one photo never advances: `goto` to the same
offset reloads, which would blank the screen every interval.

The interval is read once per start. The countdown does not wait for that read or for the
fullscreen switch, so neither can hold the first photo.

## 3. Fullscreen

The window's own fullscreen through `@tauri-apps/api/window`, granted in
`capabilities/default.json` (`core:window:allow-is-fullscreen`, `allow-set-fullscreen`). The
HTML Fullscreen API was rejected: its support differs across the three webviews.

- The state before the show is remembered and restored, so a user already fullscreen stays so.
- A stop that lands while the switch is in flight finds nothing recorded to restore; the late
  switch undoes itself (a `run` stamp taken at start).
- A refused or failing switch leaves a windowed slideshow.
- **The trap:** `tauri-plugin-window-state` persists FULLSCREEN, so quitting mid-show reopens
  photon fullscreen with no title bar. Rather than fight the plugin's save ordering, photon
  gains a global `F11` toggle, which is worth having anyway, and the README says so.

## 4. Crossfade

During a slideshow `goto` parks the outgoing photo's URL in an `<img>` layered between the
stage and the controls. It stays opaque until the successor reports shown, then fades over
600 ms: no cut through black. It is skipped when the photo is zoomed or rotated, since the layer
knows nothing of either. The effect that starts the fade reads `outgoing` untracked — `goto`
sets it while the old `fullSrc` is still in place, and a tracked read would fade the old photo
before the new one was requested. The layer is removed by a timer, not `transitionend`, which
never fires when a preloaded photo decodes within the frame the layer was inserted in.

## 5. Testing

The factory is covered with fake timers (countdown gating, pause/resume, manual steps,
fullscreen restore, the late switch, refusal, chrome idling) and each rule was mutation-probed;
one probe passed at first and exposed a missing test for starting on a still-loading photo.
The interval's clamp on read and write is tested in `settings.rs`. The crossfade, keys and
Settings field are component wiring: three README checklist lines.
