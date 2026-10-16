# photon native UI - the switch-over

Sub-project 8 of `2026-10-09-photon-native-ui-design.md`: "A new screenshot harness and the
website's images, the smoke checklist and CLAUDE.md rewritten, `ui/`, `photon-app` and npm
deleted, the whole-branch review, the merge. The release that carries it is a minor at
least."

Written 2026-10-16, on the user's word ("start the switch-over after #219 is merged", then
"go ahead"), after sub-projects 1 to 7 had merged into `native-ui` and the user had run the
native window on Windows (video, the clipboard, Reveal and Open "work fine"; the People
page's wheel and scrollbar, "Add folder…" and the name suggestions came of that run, #219).
**It stops before `native-ui` is merged into `main` and before a release is tagged**: both
are asked of the user first.

## Where parity stands

The spec's four lists, read on `native-ui` at 33b694b:

- **Every key** of `lib/shortcuts.ts` is a row of `shortcuts.rs`, and the tripwire holds the
  native handlers to the native list. Nothing to port.
- **Every public function of `commands.rs`** is called by `photon-ui` but eight, none of them
  a feature: `grid_rows`, `grid_offset_of_item`, `grid_offset_of_folder` and
  `grid_folder_ids_at` page the grid to a web view, which this window holds whole;
  `scanning_folders` answers a race this window does not have; `set_star` is one photo of
  `set_stars`; `parse_key` and `clamp_count` are helpers. They go with `photon-app` where
  nothing else calls them, and the parity test (below) names any that stay.
- **Every item of the smoke checklist** written for the Svelte photon (lines 1-635, 382
  items) was read against the native code, by four readers a quarter each. What they found
  is the next section.
- **Every surface of the screenshot harness**: the native pictures have no tile sizes, no
  Videos grid, no Duplicates, no empty folder, no header menu or pinned header, no saved
  search, no sort list and no video in the viewer. PR 4 draws them.

## What the audit found, and what is done with it

**Ported** - each a difference a user sees:

1. Closing the viewer keeps a selection of several photos, where it was opened from one of
   them and closed on it (`selectItem`/`closeViewerOn` in `library.svelte.ts`). Today the
   selection collapses to the photo, and Shift+End, Enter, Escape loses the range.
2. A tile's marks - the star, the copies mark, a video's play glyph and its length - have
   the drop shadow of `--shadow-ink` (`Tile.svelte`): on a bright photo the white ones
   vanish.
3. The tiles of a folder that is offline are dimmed (`Grid.svelte` `dimmed`, opacity 0.4).
4. A thumbnail fades in as it arrives (`tile-fade.ts`), where it now appears at once.
5. A video is fitted above its strip and the bar, not under them (`Viewer.svelte`'s
   `video.full`).
6. The search help closes when the keyboard leaves the box by Tab (`SearchBar.svelte`'s
   focusout).
7. An open keyword view follows a rename of its keyword in Settings: the engine moves its
   argument outside the views' queue, and `Nav` may keep the old one - to be proved by a
   test first, and fixed if it fails.
8. Keyboard focus is drawn where a press can go: the sidebar's rows, the empty library's
   buttons and the top bar's size buttons draw the ring the Settings controls draw; a
   sidebar row given the keyboard is brought into sight; a rename closed from the keyboard
   gives the keyboard back to its row (`backToRow`).
Two more that the first person to run the window on Windows found while this was written,
and that no line of the Svelte code names, each being the web view's own: **the search
box's cross**, which empties it (a `type="search"` input draws one), and **the whole path
under the pointer** in the info panel, where a long one is cut short (its `title`).

9. **Dates and numbers in the system's language and order** (the user's answer, over
   English-only): a period's header, a folder's month, the viewer's caption, the info
   panel's dates, "On this day" and every count with thousands, as the Svelte photon's
   `Intl` writes them, and folder names compared as `Intl.Collator` compares them
   (`numeric`, base sensitivity). Through ICU4X (`icu`, pure Rust, its compiled data -
   within the rule against C dependencies, at the cost of some megabytes in the binary),
   the locale read from the system (`sys-locale`). The words photon writes itself stay
   English, as in the Svelte photon.

**Left, as rulings** (each said in CLAUDE.md or the checklist where it is not already):

- No folder dropped on a Wayland window: winit 0.30 has no drops there. The empty
  library's line and the card are left out on Wayland, as they are today. Revisit with
  winit.
- The info panel's text cannot be selected; its file name is copied by a click on the
  caption, as before. Ctrl+C there is the photo.
- A held Ctrl+C copies again as each copy ends: egui hands a copy over as an event with no
  repeat to tell it by.
- A touchscreen's flick over the grid draws a band, where a browser pans: the grid reads
  no touch scrolling. Not seen on a touchscreen; on the checklist.
- A face's outline is no stop of the Tab key (the viewer's keys would be its); naming a
  face from the keyboard goes through the viewer's menu.
- Where photon cannot open its library on a Linux without `zenity`, the reason is on the
  standard error and in the log, with no window.
- The dropdown tells a screen reader its chosen option, not the one the keys are on.
- A photo hidden in the viewer and the viewer closed leaves what was selected before it
  selected, where the Svelte store leaves nothing (`closing_on_a_photo_the_view_has_lost_
  selects_nothing_in_its_place` holds it).
- Ctrl+B under an open menu does nothing (the menu takes every key); closing Settings
  gives the keys to the grid, not to the gear; the zoom's slider takes no keyboard; the
  export's size is five buttons; a keyword appears in the panel when the library has it.
- Compare makes every pane's full picture once zoomed, the pane with the keys first, where
  the Svelte compare made one.

## The pull requests

1. **Parity: the grid, the viewer and the keyboard** - items 1-8 above, and this spec.
2. **Dates and numbers in the system's locale** - item 9: `locale.rs`, the formatters it
   replaces (`grid/labels.rs`, `viewer_caption.rs`, `viewer_info.rs`, the counts), and the
   folder names' collation.
3. **photon-native becomes photon, and the release is the native one** - the binary is
   `photon`; `release.yml` builds the six installers by `xtask package` on a tag, as a
   draft release with `SHA256SUMS`, and `native-packages.yml` is folded into it (artifacts
   on `native-ui` until the merge); the icons move into `photon-ui`; `xtask versions` and
   `metadata` read no `tauri.conf.json`.
4. **The pictures** - `native-shot` draws the surfaces the Svelte harness drew and it
   lacks, and with `--photos` draws them over the CC0 photos for the website; `site/img`
   is made from it.
5. **The Svelte photon goes** - `ui/`, `crates/photon-app`, `package.json`,
   `package-lock.json`, the npm gate, the Chromium screenshot harness and `mock.js`,
   `scroll-probe`, and the grid gate's Svelte column (`svelte-probe.patch`, its worktree):
   with nothing to measure against, the gate goes, and `--probe` with it unless the
   measurement is kept on its own. CI installs neither webkit2gtk nor Node. The engine's
   functions no one calls go, and a test fails on a public function of `commands.rs` that
   `photon-ui` never calls. CLAUDE.md and the smoke checklist are written again for one
   interface: what was the Svelte photon's alone goes, what the native window answers
   stays under its own name.
6. **The whole-branch review** - one reader of `main..native-ui` entire, pointed at what
   the native window arms in the engine and at the release; one fix pass.

Then the user is asked: the merge of `native-ui` into `main`, the version (a minor at
least), and the release notes, which say that the interface is new, that the library is
the same file (no schema change in this sub-project), that `window.ron` and `layout.json`
are new beside it, and that the MSI replaces the Tauri photon's.

## Verification

The Rust gate at every commit; every new test shown to fail with its rule broken. The
locale is tested with locales given, not read from the machine. The pictures are read,
not launched. The installers are the workflow's. Nothing here writes outside `library.db`
but what the sub-projects before it write.
