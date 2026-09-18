# photon — Search Grammar Design

**Date:** 2026-09-18
**Status:** Approved design, implemented
**Supersedes:** the OR decision in `2026-09-16-photon-multi-token-search-design.md`
**Builds on:** v0.14.2

## 1. Why the OR goes

Since 2026-09-16 the words of a query were OR-ed, for the "I remember a lake and a bell"
query. That cannot coexist with a term meant as a filter: under OR, adding `2019` or a
camera to a query returns more photos, not fewer. The user asked for filters (dates, camera,
lens) and chose to drop the automatic OR rather than special-case them. Widening stays
available, but it is asked for.

Search already matched the camera, the lens, keywords and the capture date as `YYYY-MM-DD`
(2026-09-16 metadata spec), so `2019` and `2019-07` needed nothing new: a date-shaped word
matches the date *or* a name containing it, which is the behaviour the user chose. What this
design adds is the grammar, the field prefixes, and the info panel's links.

## 2. The grammar

- Adjacent words are AND-ed. Each word may match in a different field: `italy lake` finds
  `lake.jpg` in `2019 Italy/`.
- `OR` widens. `AND` is accepted and means what adjacency means. `AND` binds tighter:
  `lake bell OR pond` is (lake AND bell) OR pond. No parentheses.
- Operators are **capitals only**. `salt and pepper` searches for the word "and".
- `"double quotes"` make one term of several words and make an operator or a prefix literal.
  An unclosed quote runs to the end of the input.
- `camera:` and `lens:` (any case) confine a term to the make-and-model, or to the lens. A
  quoted value of several words asks for **every word** in the field, not the phrase: the
  info panel shows "NIKON D750" for make "NIKON CORPORATION", model "NIKON D750", and a
  phrase match would depend on how each maker spaces its own name.
- Dangling pieces are ignored, not searched for: an operator with nothing on one side, a
  prefix with no value. They are what a query looks like between keystrokes; reading
  `lake OR` as "lake AND the word or" would flash an empty grid while typing.
- A query holding only operators is empty and matches nothing. The engine, which calls only
  whitespace blank, stays in Search with no results — coherent with the text in the box.

The parsed form is a list of alternatives, each a list of terms (`Any`, `Camera`, `Lens`).
Matching stays in Rust, for the reasons `search.rs` already records.

## 3. The info panel

The Camera and Lens rows are buttons. Clicking one closes the viewer and runs
`camera:"<shown name>"` / `lens:"<lens>"` through the search box (`searchBox.search`), so the
box shows what the grid is filtered by. A pending debounced send is cancelled first, or
half-typed text would land afterwards and replace the link's query. A `"` inside a value is
dropped, since the grammar has no escape.

No new view, no sidebar section, no IPC change: the query string is the whole interface.

## 4. Testing

Unit tests in `search.rs` pin each rule; `library::items` pins that the words reach across
fields and that make, model and lens reach `Fields`. Probed by reverting: AND to OR, the
`Fields` wiring, capitals-only operators, and `search()`'s cancel each fail a test. The
button wiring in `Viewer.svelte` is a component and is on the README checklist.
