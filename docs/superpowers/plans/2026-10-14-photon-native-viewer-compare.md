# The native viewer, 8 of 8: compare

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Interfaces and
> tests are given, not every line: the plan is executed by its author, in one session.

**Goal:** two to four photos side by side in the native window, at one zoom over one
point, each saying what distinguishes it from the others - from `C` in the grid and the
tile's "Compare N photos".

**Architecture:** `compare.rs` (state) is `createCompare` and `differingFacts`: the panes,
the pane with the keys, and the one zoom - the viewer's own `Zoom`, measured in a pane's
size. `compare_view.rs` draws the panes in two columns and answers the keys, the wheel and
a drag as `CompareAction`s. `app.rs` asks what each photo is off the UI thread before
anything is shown (`App::comparing`), draws compare over the whole window as it draws the
viewer, gives the panes their previews, and once the zoom has left the fit asks for each
pane's full picture in turn (`App::compare_pictures`).

**Spec:** `docs/superpowers/specs/2026-10-12-photon-native-viewer-design.md` ("Compare").

## Global constraints

- No SQLite and no decode on the UI thread. The one write is a star, the folder's
  `.picasa.ini`, through the engine's own command.
- State modules name no egui type and are listed in `STATE_MODULES`; the gate at every
  commit; every new test shown to fail with its rule broken.

## Rulings made in the plan

- **Every pane's full picture once zoomed**, the spec's rule, where the Svelte compare asks
  only for the focused pane's: the native window holds the pictures as textures and makes
  them one after another, so the panes fill in one by one rather than waiting behind each
  other in a queue of renders. Fitted, a pane is its preview, which is about a pane's size;
  no copy fitted to a box is made for a pane.
- **The wheel zooms by itself**, as in the Svelte compare (no Ctrl): there is nothing to
  step to. A notch is a factor of 1.15; a trackpad's points add up, a hundred to a notch.
- **A star is the library's**: pressed, it is written, and the pane says so once the grid's
  rebuild says it - where the Svelte compare flips it at once and puts it back on failure.
- **Enter opens the pane with the keys in the viewer** and closes compare; the viewer then
  goes on as it does from the grid.
- **Compare opens when the library has said what every photo is**: half a comparison is
  worse than none, and a refusal is said and opens nothing.
- **Escape closes; while a drag is held, Escape lets go of the drag and does nothing
  else**, and no other key is answered - the band's rule.
- What the Svelte compare says over a pane whose preview could not be loaded is not said:
  the pane stays blank.

## Probes

Twenty-six, each rule broken by an exact replacement and its test run: twenty-two failed as
stated. The four that passed, each settled: a press at fit was not seen to hold nothing (it
would have held the keys for the drag), and a press on the cross over a pane not seen to
give the pane no keys - both tested now; the count asked of before the library is read had
no test (`C` with the whole library selected would read every photo of it) - counted now,
`App::compares_asked`; and a box of nought made into no box did nothing, `fitted_copy`
making no copy for one already - taken out.
