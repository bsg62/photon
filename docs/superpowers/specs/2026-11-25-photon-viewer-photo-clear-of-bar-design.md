# The viewer's photo kept clear of the bar (design)

A small change. Asked for 2026-11-25.

## Why

The viewer's bar (star, turns, crop, info panel, caption) floats over the photo at the
window's foot, and covers the bottom of every photo that fills the window's height. The
user wants a setting that keeps it from covering the photo.

## Approaches

- **A. Move the bar to the top** - what was first asked for. Rejected: it covers the top
  of a tall photo instead of its bottom; the problem moves, it does not go.
- **B. Fit the photo above the bar** - picked. Nothing is ever covered; a photo that
  fills the height is about 62 points (`EDGE + BAR + EDGE`) smaller. The viewer already
  does this for a video, whose picture stops above its strip (`fit_in` in
  `viewer_view.rs`).
- **C. Hide the bar when the pointer rests** - rejected: the tools need a mouse move to
  reach, and the slideshow's controls already behave so; it hides the bar, not the
  covering.

## The design

- **The setting.** Settings → Appearance, a checkbox under "Animations": *"Keep photos
  clear of the viewer's bar"*. Off by default, so nobody's viewer changes on upgrade.
  Stored as a library setting beside `animations` (`photon_core::library::settings`, a
  key and a getter/setter, `commands::` pair in the engine), read off the UI thread like
  the animations flag and held by the application as a window flag.
- **What it does.** With it on, the photo's area (`viewer_layout::photo_area`) stops
  `EDGE + BAR + EDGE` above the window's foot, so the bar and the zoom slider in the
  corner beside it stand in black. Everything measured in the area follows by itself: the
  fit, the fitted copy the engine is asked to scale, the zoom's bounds and the pan, the
  faces' outlines and their cut, a message in the picture's place, the opening and
  closing passage (which lands where `picture()` draws). A zoomed photo is cut at the
  area's foot, so it stops at the bar too.
- **A video** already stops above its strip, which stands above the bar; nothing changes
  for it.
- **The info panel** keeps its place (its foot already stands clear of the bar).
- **The prev/next buttons** stay at the middle of the window's height, as they are.
- **The photo's own caption gets room too** (the user's choice). It is the caption a
  photo carries (`caption_place`), up to two lines standing `CAPTION_BOTTOM` above the
  foot, over the photo today. With the setting on, the area stops above the caption's top
  when the photo has one and it is drawn (not while the info panel is open, which has the
  caption whole), so a captioned photo is fitted a line or two smaller than one without.
- **No jump at a step.** The caption is known a round trip after a step (the library's
  answer), so until it is, the room kept is the room the last photo had - as the bar keeps
  its width (`bar_place`'s `was`). Stepping between photos alike in having a caption, the
  photo does not move; stepping from one with to one without (or a one-line to a two-line
  caption), it resizes once, when the caption is known, and the opening passage follows a
  landing that moves (`Passage::follow`) instead of jumping.

## Builder's calls (mine, for the user to check)

- **A slideshow keeps the whole window.** Its bar is gone at rest and comes back over the
  photo when the mouse moves; fitting the photo to the bar there would make the photo jump
  each time the controls come and go. With the setting on, a show is drawn as today.
- **The crop tool follows the setting.** Its box is fitted to the whole window today
  (`ViewerView::cropping`'s `whole`), so its bar covers the bottom edge and handles of a
  tall photo; with the setting on, the box is fitted into the same area above the bar.
- **The wording** of the checkbox and whether Appearance's hint line mentions it.

## Not in this change

Moving the bar to the top; hiding the bar outside a show.
