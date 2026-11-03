# More Animations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The viewer zooms out of the photo's tile and back into it, and menus, dropdowns and
dialogs fade in growing from 92% and fade out, all behind the existing `animations` switch.

**Architecture:** Arithmetic in egui-free state modules: `animate.rs` gains the eased
progress of any duration and the growth's scale, and a new `viewer_motion.rs` holds the
photo's passage between its tile and the viewer (where it is drawn, its crop, the black,
the controls). Two egui pieces carry the drawing: `afterimage.rs` keeps the shapes a frame
drew on some layers and paints them again later at an opacity (the grid frozen under an
opening viewer, a menu or dialog fading out), and `Ui::with_visual_transform` scales a menu
or dialog while it grows without touching its hit tests. The stored state flips at the press
everywhere; the motion only draws it.

**Tech Stack:** Rust, egui/eframe 0.36.2 (pinned exactly; read
`~/.cargo/registry/src/*/egui-0.36.2/` - `Ui::with_visual_transform` at `ui.rs:2746`,
`GraphicLayers`/`PaintList` in `layers.rs`, `Modal` in `containers/modal.rs`), egui_kittest.

**Spec:** `docs/superpowers/specs/2026-11-03-photon-more-animations-design.md`

Work on a branch `more-animations` off `main`.

## Global Constraints

- Opening: 200 ms (`animate::DURATION_MS`) on `animate::ease_out`, for the viewer, menus,
  dropdowns and dialogs. Closing a menu, dropdown or dialog: 100 ms fade, same curve, no
  shrink. The viewer's controls fade in over 100 ms once the photo has landed.
- Menus, dropdowns and dialogs grow from 92% (`GROW_FROM = 0.92`) to 100%.
- Progress is the window's clock in ms (`ui.input(|i| i.time) * 1000.0`), never frames.
- A still window draws nothing: frames are asked (`request_repaint`) only while a motion
  runs; `driver.frames_in(&mut harness, 1.0) == 0` once it has ended.
- The stored state flips at the press: `App::viewer`, `App::menu`, `App::dialog` are
  `Some`/`None` from the press, and every existing rule keeps reading them.
- Hit tests are the full-size, final places from the first frame. A growing menu or dialog
  is pressed where it will stand; a fading one takes no press, hover or key.
- `animations` off (`App::without_animations`, or the Settings switch): everything appears
  and goes in one frame, as on `main`.
- egui's own area fade stays off (`fade_in(false)`); the motion is photon's.
- Not covered: the People page, compare, the crop tool, toasts, the drop card, the search
  box's help panel, a step in the viewer, the slideshow's crossfade.
- New state modules go into `STATE_MODULES` in `lib.rs` and name no egui type.
- Whole-application tests launch without animations; a motion's test says so
  (`launch_animated`, `opened_animated`, `over_animated`, `three_animated` in
  `tests/viewer.rs`).
- The gate before every commit (CLAUDE.md): `cargo fmt --all`, `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo nextest run --workspace`,
  `cargo bench -p photon-core --bench grid --no-run`.
- Every new test is shown to fail with its change reverted (exact replacement, restore,
  `touch` the file). A probe that passes is a finding.

## Review Focus

1. **Escape in the middle of the opening zoom** - the photo turns round from where it is
   drawn and shrinks into the tile; no frame shows the full black over the grid or a jump.
   (Task 6: `escape_while_it_grows_turns_it_round`.)
2. **Keys pressed during the zoom** - an arrow steps at once (the viewer's keys are its own
   from the press) and the zoom lands there and then; a key pressed while it shrinks is the
   grid's. (Task 5: `the_viewers_keys_are_its_own_from_the_press`; Task 6:
   `the_grid_is_the_grids_while_the_photo_shrinks`.)
3. **A menu opened at the bottom-right corner** - placed flipped, grows out of the corner at
   the click, and an item pressed in its first frame at its final place is picked. (Task 2:
   `a_menu_flipped_at_the_corner_grows_from_the_click`,
   `a_growing_menu_is_pressed_at_its_full_size`.)
4. **A click right after a menu or dialog closes** - lands on what is under the fading one
   (the existing `overlay_closed` rules aside). (Task 2:
   `a_closed_menu_fades_and_takes_no_press`.)
5. **The window resized during the zoom** - the photo lands where the viewer now fits it,
   with no jump at the end. (Task 5: `a_resize_during_the_zoom_lands_where_the_viewer_fits_it`.)

---

### Task 1: Eased progress, the growth, and afterimages

**Files:**
- Modify: `crates/photon-ui/src/animate.rs`
- Create: `crates/photon-ui/src/afterimage.rs` (a view module: it names egui types)
- Modify: `crates/photon-ui/src/lib.rs` (`pub mod afterimage;`)

**Interfaces:**
- Produces, in `animate.rs`:
  - `pub const FADE_OUT_MS: f64 = 100.0;`
  - `pub const CONTROLS_MS: f64 = 100.0;`
  - `pub const GROW_FROM: f32 = 0.92;`
  - `pub fn eased(since_ms: f64, duration_ms: f64) -> f64` - `ease_out(since / duration)`,
    0 before the start, 1 after the end.
  - `pub fn grown(k: f64) -> f32` - `GROW_FROM + (1 - GROW_FROM) * k`.
- Produces, in `afterimage.rs`:
  - `pub struct Afterimage { shapes: Vec<egui::epaint::ClippedShape>, started_ms: f64, duration_ms: f64 }`
  - `pub fn of_layers(ctx: &egui::Context, keep: impl Fn(egui::LayerId) -> bool, now_ms: f64, duration_ms: f64) -> Afterimage`
    - takes the shapes of the layers `keep` accepts, in paint order (layer ids from
      `ctx.memory(|m| m.layer_ids())` *first*, then `ctx.graphics`; never one inside the other's
      closure: both lock the context).
  - `pub fn paint(&self, painter: &egui::Painter, opacity: f32)` - each shape through
    `painter.with_clip_rect(shape.clip_rect)` after `set_opacity(opacity)`.
  - `pub fn opacity(&self, now_ms: f64) -> f32` - `1 - eased(now - started, duration)`.
  - `pub fn running(&self, now_ms: f64) -> bool`.

- [ ] **Step 1: Write the failing tests**

In `animate.rs`'s tests:
```rust
#[test]
fn eased_is_the_slides_curve_over_any_time() {
    assert_eq!(eased(50.0, 100.0), 0.875);
    assert_eq!(eased(-5.0, 100.0), 0.0);
    assert_eq!(eased(250.0, 100.0), 1.0);
}

#[test]
fn a_growth_starts_at_ninety_two_hundredths() {
    assert_eq!(grown(0.0), 0.92);
    assert_eq!(grown(1.0), 1.0);
}
```
In `afterimage.rs`'s tests, running a bare `egui::Context` twice:
- `an_afterimage_paints_what_its_layers_held_at_the_opacity_asked`: frame 1 paints a
  `Color32::RED` rect on `LayerId::new(Order::Foreground, Id::new("a"))`; `of_layers` keeps it;
  frame 2 paints it with `opacity 0.5` on another layer; the frame's output holds one rect
  shape whose fill is `Color32::RED.gamma_multiply(0.5)`, with frame 1's clip rect.
- `an_afterimage_leaves_out_the_layers_it_is_not_told_to_keep`: two layers painted, one kept,
  one shape painted again.
- `an_afterimage_fades_over_its_time`: `opacity` 1 at start, `1 - 0.875` at half, 0 and not
  `running` at the end.

- [ ] **Step 2: Run them and see them fail** - `cargo nextest run -p photon-ui --lib animate:: afterimage::` - does not compile (names missing).
- [ ] **Step 3: Implement** the two constants, `eased`, `grown`, and `Afterimage`.
- [ ] **Step 4: Run them and see them pass**, then the gate.
- [ ] **Step 5: Commit** - `feat(ui): eased progress of any length, and afterimages of drawn layers`

---

### Task 2: Menus and dropdowns grow in and fade out

**Files:**
- Modify: `crates/photon-ui/src/menu_view.rs` (`MenuView`, `show_in`, `AREA` layer)
- Modify: `crates/photon-ui/src/select_view.rs` (`SelectData`, `SelectView`)
- Modify: `crates/photon-ui/src/controls_bar.rs`, `viewer_view.rs` (`SelectData { animations, .. }`)
- Modify: `crates/photon-ui/src/app.rs` (menu drawing at ~line 5041, `open_menu`, the end of
  `ui` and its early returns, test accessors)
- Modify: `docs/architecture/ui-shell.md` (the motion paragraph), `docs/smoke-checklist.md`
- Test: `crates/photon-ui/tests/menu.rs`

**Interfaces:**
- Consumes: `animate::{eased, grown, DURATION_MS, FADE_OUT_MS}`, `afterimage::{Afterimage, of_layers}`.
- Produces:
  - In `menu_view.rs`: `pub struct Growth { pub rect: Rect, pub origin: Pos2, pub scale: f32, pub opacity: f32 }`
    (the full-size rectangle, the point it grows from, and what was drawn this frame);
    `pub fn layer() -> egui::LayerId` (the menu's area layer);
    `MenuView::show_in(&mut self, ui, at, items, palette, animations: bool) -> Outcome<A>`
    (new last parameter; `show` passes `false`); `MenuView::growth(&self) -> Option<Growth>`.
  - In `select_view.rs`: `SelectData.animations: bool`; `SelectView::growth(&self) -> Option<Growth>`.
  - On `App`: `pub fn menu_growth(&self) -> Option<Growth>`,
    `pub fn list_growth(&self, id: &str) -> Option<Growth>` (a dropdown's, by its `SelectData.id`),
    `pub fn fading(&self) -> usize` (afterimages being painted).

**Rules:**
- The growth's start is the first frame the menu is drawn after `reset` (the time kept in
  `MenuView`, as `DialogView::opened_at` keeps a dialog's). With `animations` off the scale
  and opacity are 1 from the first frame.
- The origin: the click `at`, clamped into the placed menu's rectangle - so a menu
  `place_menu` flipped grows out of its corner at the click. A dropdown's: the middle of its
  list's top edge (bottom edge for `above`).
- Inside the area's closure, the menu is drawn in
  `ui.with_visual_transform(TSTransform::new(origin.to_vec2() * (1 - s), s), ..)` after
  `ui.multiply_opacity(k)`, with `s = grown(k)`, `k = eased(now - opened, DURATION_MS)`.
  At `k == 1` no transform is applied (the text is drawn on whole pixels).
- A menu asks for a repaint while `k < 1`.
- Closing: wherever `self.menu` goes from `Some` to `None` in a frame the menu was drawn in,
  the app takes `of_layers(ctx, |l| l == menu_view::layer(), now, FADE_OUT_MS)` and paints
  it every frame on `LayerId::new(Order::Middle, Id::new("fading"))` at `opacity(now)` until it
  is done (one `App::fading: Vec<Afterimage>`, painted and pruned at the start of each frame's
  drawing, repaint asked while any runs). A menu closed in a frame it was not drawn in goes at
  once. The `Middle` order keeps the fading menu under a new one opened by the same right
  click, and the afterimage is not an area: it takes no press.
- A dropdown keeps its own afterimage in `SelectView` (it knows when its list closes) and
  paints it the same way.

- [ ] **Step 1: Write the failing tests** in `tests/menu.rs`, launched with `opened_animated`:
  - `a_menu_grows_from_where_it_was_opened_and_then_asks_for_nothing`: right-click a tile at
    `p`; in the first frame `growth.origin == p`, `growth.scale < 0.93`, `growth.opacity < 0.2`;
    at `+0.1 s` `0.98 < scale < 1.0`; at `+0.2 s` `scale == 1.0` and `opacity == 1.0`; then
    `driver.frames_in(&mut harness, 1.0) == 0`.
  - `a_menu_flipped_at_the_corner_grows_from_the_click`: right-click at the grid's bottom-right
    tile corner; `growth.rect.max` within 1 pt of the click (it opened up and left) and
    `growth.origin == growth.rect.max`.
  - `a_growing_menu_is_pressed_at_its_full_size`: in the frame after the right-click, click
    "Star photo" by its accessible label (kittest query: its node is the full-size place);
    the photo is starred.
  - `a_closed_menu_fades_and_takes_no_press`: Escape at `p`; `menu_open()` false at once;
    `fading() == 1` at `+0.05 s`, `0` at `+0.1 s`; a click on another tile at `+0.03 s` under
    where the menu stood selects that tile.
  - `a_dropdown_grows_down_from_its_control`: press the sort control; `list_growth("sort")`
    origin at the middle of the list's top edge, `scale < 0.93` in the first frame, `1.0` at
    `+0.2 s`.
  - `without_animations_a_menu_is_there_at_once`: with `opened`, `scale == 1.0` and
    `opacity == 1.0` in the first frame; Escape leaves `fading() == 0`.
- [ ] **Step 2: Run and see them fail** - `cargo nextest run -p photon-ui --test all menu::`.
- [ ] **Step 3: Implement** per the rules above.
- [ ] **Step 4: Run and see them pass**; probe each (no transform; origin at the menu's
  top-left instead of the click; afterimage not taken; afterimage painted on an `Area`) and see
  the matching test fail.
- [ ] **Step 5: Notes.** Extend `ui-shell.md`'s motion paragraph: menus and dropdowns grow by a
  visual transform (hit tests full size), the origin rule, the afterimage on `Middle` and why.
  Add to `docs/smoke-checklist.md`: "Right-click near each corner: the menu grows out of the
  pointer; Escape fades it."
- [ ] **Step 6: Gate and commit** - `feat(ui): menus and dropdowns grow in from the pointer and fade out`

---

### Task 3: Dialogs grow in and fade out

**Files:**
- Modify: `crates/photon-ui/src/dialog_view.rs` (`show`, `layer()`)
- Modify: `crates/photon-ui/src/app.rs` (`show_dialog`, closing, accessors)
- Modify: `docs/architecture/ui-grid.md` (the dialogs' section), `docs/smoke-checklist.md`
- Test: `crates/photon-ui/tests/dialogs.rs`

**Interfaces:**
- Consumes: Task 1; `menu_view::Growth`; `App::fading` from Task 2.
- Produces: `DialogView::show(&mut self, ui, dialog, keywords, tags, people, animations: bool) -> Done`
  (new last parameter); `DialogView::growth(&self) -> Option<Growth>`; `pub fn layer() -> egui::LayerId`
  (the `Id::new("dialog")` area, `Order::Foreground`); `App::dialog_growth(&self) -> Option<Growth>`;
  `App::backdrop(&self) -> Option<egui::Color32>` (the backdrop drawn this frame).

**Rules:**
- `k = eased(now - opened_at, DURATION_MS)` from `DialogView::opened_at` (already kept).
  The dialog's own drawing (shadow, ground, contents) goes inside
  `ui.with_visual_transform` about the middle of the rectangle it draws in this frame
  (`ground`), with `ui.multiply_opacity(k)`. The backdrop is `Modal`'s, outside that closure:
  `.backdrop_color(color(SCRIM).gamma_multiply(k))`, so it fades and never scales.
- A picker that grows downwards from where it would stand at its tallest grows from the
  middle of what it draws in its first frame (kept with `opened_at`).
- Closing: as a menu's, with `dialog_view::layer()` - the afterimage holds the backdrop too,
  so the two fade together. `App::overlay_closed` is set as now.

- [ ] **Step 1: Write the failing tests** in `tests/dialogs.rs` with `over_animated`:
  - `a_dialog_grows_from_its_middle_and_its_ground_fades_in`: `?` opens the shortcut sheet at
    `p`; first frame `growth.scale < 0.93`, `growth.origin == growth.rect.center()`,
    `backdrop().a() < color(SCRIM).a() / 4`; at `+0.2 s` scale 1, the backdrop is `SCRIM`;
    then no frames.
  - `a_closed_dialog_fades_with_its_ground`: Escape; `dialog()` is `None` at once;
    `fading() == 1` until `+0.1 s`, then 0.
  - `a_growing_dialog_is_pressed_at_its_full_size`: open the export dialog and click
    "Cancel" by its label in the next frame; it closes.
  - `without_animations_a_dialog_is_there_at_once`.
- [ ] **Step 2: Run and see them fail** - `cargo nextest run -p photon-ui --test all dialogs::`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, pass, probe** (backdrop inside the transform; no afterimage).
- [ ] **Step 5: Notes** in `ui-grid.md`'s dialogs section; a smoke line ("Open Settings and
  Escape: it grows from its middle and fades").
- [ ] **Step 6: Gate and commit** - `feat(ui): dialogs grow in from their middle and fade out`

---

### Task 4: The photo's passage, as arithmetic

**Files:**
- Create: `crates/photon-ui/src/viewer_motion.rs` (state module)
- Modify: `crates/photon-ui/src/lib.rs` (`pub mod viewer_motion;`, its `STATE_MODULES` entry)
- Modify: `crates/photon-ui/src/grid/tile.rs` (`cover_uv` returns `crop_uv((w, h), (1.0, 1.0))` as a `Rect`; its tests unchanged)

**Interfaces:**
- Consumes: `viewer_layout::Frame`, `animate::{eased, DURATION_MS, CONTROLS_MS}`.
- Produces:
  ```rust
  pub type Uv = (f32, f32, f32, f32); // left, top, right, bottom in 0..=1
  pub enum Way { Zoom, Fade }
  pub struct Passage { /* id, way, opening, from: Frame, from_uv: Option<Uv>, from_ground: f32, started_ms */ }
  impl Passage {
      pub fn opening(id: i64, way: Way, from: Frame, from_ground: f32, now_ms: f64) -> Self;
      pub fn closing(id: i64, way: Way, from: Frame, from_uv: Option<Uv>, from_ground: f32, now_ms: f64) -> Self;
      pub fn id(&self) -> i64;
      pub fn way(&self) -> Way;
      pub fn is_opening(&self) -> bool;
      pub fn photo_at(&self, to: Frame, now_ms: f64) -> Frame;
      pub fn uv_at(&self, image: (f32, f32), drawn: Frame, now_ms: f64) -> Uv;
      pub fn ground(&self, now_ms: f64) -> f32;
      pub fn controls(&self, now_ms: f64) -> f32;
      pub fn landed(&self, now_ms: f64) -> bool;
      pub fn running(&self, now_ms: f64) -> bool;
      pub fn fade_instead(&mut self, at: Frame);
  }
  pub fn crop_uv(image: (f32, f32), frame: (f32, f32)) -> Uv;
  pub fn lerp_frame(a: Frame, b: Frame, k: f32) -> Frame;
  ```

**Rules** (`k = eased(now - started, DURATION_MS)`):
- `photo_at`: `Zoom` - `lerp_frame(from, to, k)`, `to` being the landing place when opening and
  the tile when closing, read anew each frame by the caller; `Fade` - `from` (the photo
  stays where it stands; an opening fade is made with `from` = its landing place).
- `uv_at`: `crop_uv(image, drawn's aspect)`; with a `from_uv` (a zoomed photo closing) the
  lerp from `from_uv` to that crop by `k`.
- `ground`: opening `from_ground → 1`, closing `from_ground → 0`, by `k`.
- `controls`: opening `Zoom` - 0 until `DURATION_MS`, then `eased(now - started - DURATION_MS, CONTROLS_MS)`;
  opening `Fade` - `ground`; closing - 0.
- `landed`: opening and `k == 1`. `running`: until the last of `k` and the controls is 1.
- `fade_instead(at)`: `way = Fade`, `from = at`; time and ground go on unbroken.
- `crop_uv` is the centred cut of a picture to a frame's shape; `cover_uv(256, 128)` stays
  `(0.25, 0.0, 0.75, 1.0)`.

- [ ] **Step 1: Write the failing tests** in `viewer_motion.rs`:
  - `crop_uv_is_the_whole_picture_when_the_shapes_match` (`(400, 300)` in `(4, 3)` → `(0,0,1,1)`).
  - `a_wide_picture_in_a_square_is_cut_at_its_sides` (`(256,128)` in `(1,1)` → `(0.25,0,0.75,1)`;
    and a tall one at top and bottom).
  - `an_opening_runs_from_the_tile_to_where_it_lands`: tile `(10,10,100,100)`, landing
    `(0,0,1000,750)`, started at 1000: `photo_at` at 1000 is the tile, at 1100 is
    `lerp_frame(tile, landing, 0.875)`, at 1200 is the landing; `ground` 0 / 0.875 / 1.
  - `the_controls_fade_in_once_the_photo_has_landed`: `controls` 0 at 1199, 0.875 at 1250,
    1 at 1300; `running` at 1299, not at 1300; `landed` at 1200.
  - `a_fade_stays_where_it_lands`: `photo_at` is `from` at every time; `controls == ground`.
  - `a_closing_runs_to_the_tile_and_its_ground_goes`: from ground 1 to 0; `controls` 0.
  - `a_zoomed_photo_closes_from_the_part_that_was_drawn`: `uv_at` at the start is `from_uv`,
    at the end the tile's crop.
  - `a_passage_that_loses_its_tile_fades_from_where_it_is`: closing, `fade_instead(at)` at
    1100; `photo_at` is `at` after; `ground` continuous at 1100.
- [ ] **Step 2: Run, fail** - `cargo nextest run -p photon-ui --lib viewer_motion::`.
- [ ] **Step 3: Implement**; add it to `STATE_MODULES`; make `cover_uv` use `crop_uv`.
- [ ] **Step 4: Run, pass** (with `grid::tile::` tests), gate.
- [ ] **Step 5: Commit** - `feat(ui): the arithmetic of the viewer's photo between its tile and its place`

---

### Task 5: The viewer opens out of its tile

**Files:**
- Create: `crates/photon-ui/src/app/motion.rs` (the app's side of the passage)
- Modify: `crates/photon-ui/src/app.rs` (`open_viewer`, its callers at ~2338, 3698, 5258,
  `people.rs:338/359`; `show_viewer`; test accessors)
- Modify: `crates/photon-ui/src/viewer_view.rs` (`ViewerData`, `show`)
- Test: `crates/photon-ui/tests/viewer.rs`

**Interfaces:**
- Consumes: Task 1's `afterimage`, Task 4's `Passage`, `GridView::drawn_place(id)`,
  `Thumbs::texture(key)`/`failed(key)`.
- Produces:
  - `viewer_view.rs`: `pub struct Arriving<'a> { pub passage: &'a Passage, pub now_ms: f64, pub thumb: Option<&'a TextureHandle> }`;
    `ViewerData.arriving: Option<Arriving<'a>>`;
    `pub struct PhotoDrawn { pub at: Rect, pub uv: Rect, pub texture: TextureHandle, pub image: Vec2 }`
    and `ViewerView::photo_drawn(&self) -> Option<&PhotoDrawn>` (this frame's picture, clipped to
    the photo's area, its UV cut to match - what Task 6 shrinks from).
  - `app/motion.rs`: `pub struct MotionSeen { pub photo: egui::Rect, pub ground: f32, pub controls: f32, pub opening: bool, pub zoom: bool }`;
    `App::viewer_motion(&self) -> Option<MotionSeen>` (what the last frame drew; `None` when no
    passage ran in it).
  - `App::open_viewer(&mut self, id: i64, tile: Option<egui::Rect>)` - the grid's opening passes
    `self.view.drawn_place(id)`; every other caller passes `None`.

**Rules:**
- `open_viewer` with `animations` on: takes `of_layers(ctx, keep, now, DURATION_MS)` of
  everything drawn this frame but the motion's own layers (`"fading"`, `"viewer-leaving"`) -
  the frozen picture - and a `Passage::opening`: `Zoom` from the tile when there is a tile,
  its thumbnail is held and not failed; `Fade` (from its landing place) otherwise.
  `Thumbs::rest` keeps the textures the frozen grid draws (those of its last frame).
- While the passage runs, `show_viewer` first paints the frozen picture on the background
  painter, and for `Zoom` paints the tile's rectangle over it with `palette.field` at `R[1]`
  (the tile stands empty).
- `ViewerView::show` with `arriving`:
  - `Zoom`: the ground is `Color32::BLACK.gamma_multiply(ground)`; the landing place is the fit
    it would draw now with the panel shut (`contained` in `photo_area(whole, 0.0)`) by the
    aspect of the picture it holds - its fitted copy, preview or largest level, else the
    thumbnail; the photo is drawn at `photo_at(landing)` with `uv_at`, from that picture, else
    the thumb. No faces, caption, message or controls until `controls > 0`; then they are drawn
    under `multiply_opacity(controls)` and pressed. Until then a press anywhere is nobody's
    (no pan, no half of a double click); keys are the viewer's.
  - `Fade`: the app wraps the whole `show` in a scope with `multiply_opacity(ground)`; it is
    pressed as usual.
- A step during the passage (the photo is another than `passage.id()`) ends it at once.
- Frames are asked while `running`; the frozen picture is dropped when the passage ends.

- [ ] **Step 1: Write the failing tests** in `tests/viewer.rs`, with `three_animated`:
  - `the_viewer_grows_out_of_its_tile_and_then_asks_for_nothing`: tile `t = tile_rect(0)`;
    double-click it, the second press at `p`. In the first frame `motion.zoom`,
    `motion.photo` within 2 pt of `t`, `motion.ground < 0.2`; at `p + 0.1`
    `t.width() < photo.width() < 1000.0` and `0.8 < ground < 0.95`; at `p + 0.2` the photo is the
    viewer's fit (`viewer_box()`'s rectangle) and `controls == 0.0`; at `p + 0.3`
    `controls == 1.0`; settled, `frames_in(1.0) == 0`.
  - `the_grid_stands_under_the_growing_photo_and_takes_no_press`: at `p + 0.05` a click on tile
    2's place; the viewer is still on photo 0 and the selection unchanged.
  - `the_viewers_keys_are_its_own_from_the_press`: Enter at `p`, Right at `p + 0.05`: the
    viewer is on photo 1 and `viewer_motion()` is `None` in the next frame.
  - `a_press_where_a_button_will_be_does_nothing_while_the_photo_grows`: a click on the close
    button's place at `p + 0.05` leaves the viewer open; one at `p + 0.25` closes it.
  - `a_resize_during_the_zoom_lands_where_the_viewer_fits_it`: Enter, then at `p + 0.1` the
    window to 800 × 600; at `p + 0.2` the photo is the new fit, and no frame between moved it
    by more than its share of the way.
  - `a_photo_whose_tile_has_no_thumbnail_fades_in_where_it_lands`: a fixture photo whose
    thumbnail fails (as in `grid/view.rs`'s failed-tile tests); first frame `!motion.zoom`, the
    photo already at the fit, `ground < 0.2`.
  - `without_animations_the_viewer_opens_in_one_frame`: with `three`, `viewer_motion()` is
    `None` in the first frame.
- [ ] **Step 2: Run and see them fail** - `cargo nextest run -p photon-ui --test all viewer::`.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, pass, probe** (no frozen picture; landing from the index's aspect rather
  than the picture drawn; controls pressed at once; no land on step) - each fails its test.
- [ ] **Step 5: Gate and commit** - `feat(ui): the viewer grows out of the photo's tile`

---

### Task 6: The viewer closes into its tile

**Files:**
- Modify: `crates/photon-ui/src/app/motion.rs`, `crates/photon-ui/src/app.rs` (`close_viewer`,
  `shut_viewer` and its callers at ~2037, 3400, 4699; the end of `ui`)
- Modify: `crates/photon-ui/src/grid/view.rs` (`GridData.hollow: Option<i64>`, `GridOutput.hollowed: bool`)
- Modify: `crates/photon-ui/src/settings.rs` (`ANIMATIONS`)
- Modify: `docs/architecture/ui-viewer.md` (a paragraph after "The viewer takes the whole
  window"), `docs/architecture/ui-shell.md` (the motion paragraph names the viewer),
  `docs/smoke-checklist.md`
- Test: `crates/photon-ui/tests/viewer.rs` (and `tests/slideshow.rs` for the slideshow's case)

**Interfaces:**
- Consumes: Task 5's `PhotoDrawn`, `MotionSeen`, `Passage`; `GridView::drawn_place`.
- Produces: `App::shut_viewer(&mut self, to: Option<i64>)` - `to` is the photo to shrink into
  (`close_viewer` passes the one it hands back); every other caller passes `None` (a fade).

**Rules:**
- At the press the viewer closes as now. With `animations` on and `photo_drawn()` known, the
  app keeps `Leaving { passage: Passage::closing(..), texture, image }`, `from` = the drawn
  rectangle, `from_uv` = its UV when the photo was zoomed or panned, `from_ground` = the
  ground drawn last (1, or less mid-opening). `Zoom` when `to` is `Some`, no slideshow ran and
  no People page is open; `Fade` otherwise.
- Each frame while it runs, the grid is drawn as the grid (it is the grid's: keys, presses,
  the wheel) with `hollow = Some(id)` - that tile drawn as its empty field, `hollowed` set -
  and then, on `LayerId::new(Order::Middle, Id::new("viewer-leaving"))`, the black at
  `ground` over the window and the photo at `photo_at(drawn_place(id))` with `uv_at`. A
  `Zoom` whose tile has no `drawn_place` this frame calls `fade_instead(where it was drawn)`.
- `open_viewer` while a `Leaving` runs on the same photo starts the opening from its drawn
  rectangle and ground (it turns round); on another photo the `Leaving` is dropped.
- Over the People page there is no grid: the overlay is drawn over the page, a `Fade`.

- [ ] **Step 1: Write the failing tests** in `tests/viewer.rs`, with `three_animated`:
  - `closing_shrinks_the_photo_into_its_tile_over_the_live_grid`: open photo 0, settle,
    Escape at `p`: the viewer is closed at once; first frame `!motion.opening`, `motion.zoom`,
    photo within 2 pt of the fit, `ground > 0.8`, `last_frame().hollowed`; at `p + 0.2` the
    photo is `drawn_place(id)`; then `viewer_motion()` `None`, `hollowed` false, no frames.
  - `stepped_it_shrinks_into_the_photo_it_is_on`: open 0, Right, settle, Escape: the motion
    ends at `drawn_place` of photo 1.
  - `the_grid_is_the_grids_while_the_photo_shrinks`: photo 0 open, Escape at `p`, Right at
    `p + 0.05`: the selection's lead is on offset 1 (0 handed back, then the grid's arrow).
  - `escape_while_it_grows_turns_it_round`: Enter at `p`, Escape at `p + 0.1`: the photo in the
    next frame is within 2 pt of the one drawn at `p + 0.1`; it ends at the tile.
  - `enter_while_it_shrinks_turns_it_round`: Escape at `p`, Enter at `p + 0.1`: the viewer is
    open, `motion.opening`, the photo within 2 pt of the one drawn before; it lands by
    `p + 0.3`.
  - In `tests/slideshow.rs`: `a_slideshow_closed_fades_out_where_it_stood`: the photo's
    rectangle is the same in every frame of the motion, `!motion.zoom`, ground falls to 0.
- [ ] **Step 2: Run, fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, pass, probe** (no `hollow`; target read once instead of each frame; grid
  inert while leaving; no turn-round) - each fails its test.
- [ ] **Step 5: Notes.** `ui-viewer.md`: the passage - the frozen picture under an opening
  viewer and why the grid is live under a closing one, the empty tile, the landing read from
  the picture drawn, the press rule for the controls, the fade cases and why each (no tile,
  no thumbnail, People page, compare, a slideshow's fullscreen leaving, a link, other
  results). `ui-shell.md`'s motion paragraph names the viewer's passage. Smoke checklist:
  "Open a photo from the grid, step twice, close: it grows out of its tile and shrinks into
  the one it is on; try it zoomed in, and with a portrait photo."
- [ ] **Step 6: The switch's words.** `settings::ANIMATIONS` names only the first round's
  motions; it becomes `"Animations: open, close and move things smoothly"`. The Settings
  tests that measure by it (`settings_view.rs` ~1941) read the constant and keep passing.
- [ ] **Step 7: Gate and commit** - `feat(ui): the viewer shrinks back into the photo's tile`

---

### Task 7: The whole branch

- [ ] **Step 1:** `cargo run -p xtask -- native-shot` still draws (its shots are with
  animations off); compare `native-viewer-*` and `native-menu-*` with `main`'s - unchanged.
- [ ] **Step 2:** An independent review of the branch (CLAUDE.md: a large branch), pointed at
  what the motion *arms* in old code: `shut_viewer`'s new callers, the afterimages' layers
  (nothing interactive may land on `Middle`), `Thumbs::rest` keeping the frozen grid's
  textures, and the press rules under the growing viewer.
- [ ] **Step 3:** Fix what it finds, each with its test; gate; push; open the PR.
