# photon — Timeline Scrubber Design

**Date:** 2026-09-18
**Status:** Approved design, implemented
**Builds on:** v0.14.2

## 1. What it is

A narrow year strip to the right of the grid, in every view that has folder headers. It is a
scale model of the grid's canvas: a year occupies the share of the strip its photos occupy of
the scroll, a line shows where the viewport is, hovering shows the year under the pointer,
and pressing or dragging scrolls there. A separate pure-date "Timeline" view was considered
and not chosen.

## 2. One axis

The grid is ordered by each folder's oldest photo, and the sidebar groups folders by the year
of that same value. The strip reads `Section.takenAtMin` through the sidebar's own `yearOf`,
so all three agree: pressing a printed year lands on the first header of that year, which is
the folder the sidebar lists first under it. Nothing is added to the backend or the IPC
surface; `GridInfo.sections` already carries everything.

A mark is placed wherever the year *changes*, not once per distinct year. Folder-first views
run newest to oldest, but Search places a folder by its oldest photo while `takenAtMin` is its
oldest *matching* photo, so a year can come back; it then gets a second mark, which is the
truth of what scrolling there shows.

## 3. When it shows

Only with more than one mark and a canvas taller than the viewport. Recent has no headers and
therefore no marks, which keeps it out without another `view !== 'recent'` site.

## 4. Placement and input

The grid's root becomes a flex row: the scrolling viewport, then the strip. The strip sits
outside the scroller, so it never competes with the native scrollbar, whose width differs per
platform (and overlays on macOS). It costs 44px of grid width while shown; the column count
already derives from the viewport's measured width.

Pointer capture keeps a drag scrubbing after it strays off the strip. The strip is not in the
tab order: the grid scrolls from the keyboard and the sidebar's year groups are the keyboard's
way to a year. Labels closer than 16 strip pixels to the last printed one are skipped; the
hover bubble still names them.

## 5. Testing

`ui/src/lib/timeline.ts` holds the geometry (`yearMarks`, `yearAt`, `labelledMarks`,
`scrollTopFor`), unit-tested and mutation-probed: distinct-years-only marking, no label
thinning, no end clamp and marking tile rows each fail a test. `Timeline.svelte` is pointer
wiring over those and is on the README checklist.
