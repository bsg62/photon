# The native window's remaining pages (sub-project 6)

Sub-project 6 of `2026-10-09-photon-native-ui-design.md`: "The People page, Settings,
Statistics, the shortcut sheet, duplicates and copies." What the native window draws today
in their place: the top bar's gear, drawn and taking no press; the sidebar's People heading
and its "N to name", which open nothing; "Show N duplicates" in the info panel and the
tile menu, drawn and taking no press. The Duplicates view itself works already
(sub-project 2). No `?` key.

Everything here is the Svelte UI's, ported - `Settings.svelte`, `PeoplePage.svelte` with
`people-page.svelte.ts`, `ShortcutSheet.svelte` with `shortcuts.ts`, `stats.ts`,
`copies.ts` - and every command it needs is the engine's already: **no change to
photon-core or photon-engine**, and no write photon does not make today.

## The user's decisions (2026-10-15)

- **Six pull requests**, below.
- **"Add folder…" is a typed path** until sub-project 7 brings the folder picker, as the
  export's destination is: a field checked as it is typed, with the picker's button beside
  it, in its place and taking no press.
- **Settings is a dialog**, as in the Svelte UI: the dialog layer's (`dialog_view.rs`), one
  at a time, the keys its own.
- **Reveal is in its place and takes no press** - a watched folder's and the library's in
  About - until sub-project 7, as every Reveal and Open-in-app of the native window is.

## What is built

### Settings (PRs 1 to 3)

A dialog over the whole window: a list of its nine sections on the left (Folders,
Appearance, Tags, Slideshow, Duplicates, People, Statistics, Shortcuts, About), the chosen
one on the right, scrolled by a number of its own as the info panel is. It opens from the
gear at Folders, from the empty library's "Watched folders…" and the sidebar's button
under "No folders yet." at Folders, and from the People page's "switched off" line at
People; Escape and the cross close it. It is the dialog layer's, so everything the dialogs
learnt holds: shown before anything else in a frame, every key taken, a press on the scrim
closes it, the double click that opened it presses nothing in it.

**Confirmations are made in the section, not in a second dialog** - the dialog layer has
one dialog at a time, and the Svelte UI's are the system's own `ask`, over its dialog.
Removing a watched folder, removing a tag and switching Find faces off with named people
each turn their row into the question, its words the Svelte UI's (`switchOffWarning`, the
folder's "never touches its files"), with Cancel and the action; the keyboard lands on
Cancel, as on the dialog layer's own question.

- **Appearance**: the theme (System, Light, Dark), stored and put on the window at once, as
  the top bar's control does not today; and the photo size, the top bar's own control
  again.
- **Slideshow**: the interval (1 to 60 seconds, a field that takes digits) and Shuffle,
  each stored as it changes.
- **Duplicates**: Find look-alikes - Off, Conservative, Loose - with its line of what it
  does (`set_similar_distance`, which regroups).
- **People**: Find faces, with the pass's line while it runs (`Scans::looking_for_faces`),
  and its question when it is switched off with people named.
- **About**: the version, the library's path (Reveal in its place), photon's memory read
  every few seconds while the section is shown (`memory_usage`), the licence.
- **Folders** (PR 2): every watched folder, with its status line and photo count as the
  Svelte list words them; Rescan, taking no press while that folder is scanned; Reveal in
  its place; Remove… asked in the row. "Add folder…" opens a field under the list for a
  path: a `~` at its start the home folder, checked off the window's thread as it is typed
  (`check`'s rules from the export's field: the text as it stands first), added by Enter or
  the button through `commands::add_folder`, which the drop on the window already uses
  (`Scans::added`, so an empty library does not say meanwhile that nothing is watched). A
  folder dropped on the window while the field is open is put in the field.
- **Tags** (PR 2): the library's keywords with their counts, filtered by a field; Rename in
  the row - the sidebar's field rules (`rename.rs`) - and Remove… asked in the row; the
  rename and remove rules made so far under "Changes", each with Restore.
- **Statistics** (PR 3): `stats.ts` ported (the summary line, the years, the cameras, the
  lenses, each row a bar of its share); a row is a search - Settings closes and the search
  is made as a sidebar row's is (`App::go`).
- **Shortcuts** (PR 3): the list the `?` sheet draws.

### The shortcut sheet (PR 3)

`shortcuts.ts` ported as `shortcuts.rs`: the groups and their rows, in the native window's
keys - the Svelte list less what this window does not answer, and what it answers that the
Svelte one does not. A test reads the native key handlers' sources and fails on a key a
handler answers that the list lacks, and on a listed key no handler answers, as
`shortcuts.test.ts` does. The sheet is an overlay of the dialog layer, opened by `?` in the
grid, the viewer and compare; not over another dialog, nor under a held band; it closes on
Escape, `?` and a press outside. The viewer under it is deaf, as under the person dialog.

### The Copies view (PR 4)

`GridView::Copies` with the photo's id as its argument: one photo and its copies, as its
info panel lists them. Reached from the panel's "Show N duplicates in the grid" and the tile
menu's "Show N duplicates", which take their press now; a step on the views' queue, as any
view, and the sidebar marks nothing (the Svelte UI's `copiesNotice` says what it shows, in
the line over the grid). Leaving it is any sidebar row. The view's photo leaving the
library empties it, and it says so.

### The People page (PRs 5 and 6)

Drawn **over the grid's area**, the sidebar and the top bar staying, as the Svelte page is:
the grid is not drawn under it and keeps its place, so coming back finds it where it was.
It opens from the sidebar's People heading and its "N to name"; any sidebar row, a folder,
typing in the search box and the viewer's search links leave it, as in the Svelte UI.
Opening a face's photo opens the viewer over the page; the grid is switched to All photos
only when its view lacks the photo.

`createPeoplePage` ported as `people_page.rs` (a state module, with `people-page.svelte.ts`'s
tests): its reload rules - the order of the groups on screen kept across a reload, a reload
that *started* after a write settles it, "Show more" kept across a reload at `MORE` a time,
library changes at most once a second (`CHANGE_GAP_MS`) - and its words.

**The crops are cut by the window** from the cached 1600-pixel preview, the face's
rectangle at `CROP_PX` (144) square, as `/face/` in `protocol.rs` cuts them: off the
window's thread, one cut at a time per core as `FACE_CROPS` bounds them, each preview
decoded once for all its faces (`ThumbCache`'s last eight). A crop is a texture held by its
face's id and thumbnail key - a key names one picture, so a crop kept is never stale - and
let go of when the page closes. A face whose preview is not cached shows an empty square,
and is asked for again when the page next opens - as the Svelte strip keeps a crop that
failed until it is mounted again (decided in the PR's review).

- **PR 5**: the page's sections and their first strips - groups to name, the photos with
  one face, people, ignored groups and faces - with their counts; naming a group (the name
  box, `nameChoice`'s words for a name another person has: a merge); opening a face's
  photo; the "switched off" line with its way to Settings; the pass's progress line.
- **PR 6**: a person's suggestions (Confirm, Not this person, Confirm all on screen),
  rename, merge (the merge list is a menu of `menu.rs`), delete (asked, as the dialog
  layer's question - the page is no dialog), hide and show a group or a face, Show more and
  Show fewer.

## What is not built

The folder picker, the clipboard, Reveal and Open in app (sub-project 7); window state and
the remembered fullscreen (7). Anything the Svelte pages do not do.

## Decided while writing

1. **Confirmations inside Settings are made in the row** (above): one dialog at a time.
2. **The theme is applied to the window at once**, egui's visuals and the title bar's theme
   - the title bar through `ViewportCommand::SetTheme`, the one call of it; whether the
   system's own title bar follows on each system is for sub-project 7's smoke checklist.
3. **A row of Statistics closes Settings** and searches; the Svelte UI does the same.
4. **The People page is not a `GridView`**, as in the Svelte UI (`mainPage`): it has no rows,
   and the views' queue, `Nav` and the selection know nothing of it.
5. **Crops are not cached on disk**: the webview kept them in its own cache by URL; here
   they are cut again at each opening of the page, from previews already cached.

## Pull requests

1. **Settings, the dialog**: the section list, Appearance, Slideshow, Duplicates, People
   (with its question), About; the gear and the empty library's buttons open it.
2. **Folders and Tags**: the watched list with Rescan and Remove…, the typed "Add folder…";
   tags with rename, remove and the changes.
3. **Statistics, the shortcut list and the `?` sheet.**
4. **The Copies view.**
5. **The People page**: sections, strips and crops, naming a group, opening a photo.
6. **The People page's corrections**: suggestions, rename, merge, delete, hide, more.

Each with the review every pull request of the native window has had.

## Testing

As every sub-project before it: state modules tested without a context (`people_page.rs`
with `people-page.svelte.ts`'s cases, `shortcuts.rs`, the statistics' rows and words, the
settings' fields), views in whole frames, and the whole application through `tests/common`'s
`Driver` over a library of its own making - a watched folder added by typed path and its
scan heard, a tag renamed and restored, a person named and merged. Every new test is shown
to fail with its rule broken.
