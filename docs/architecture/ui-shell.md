# The window: styling, layout, navigation, sidebar, search, status

Part of photon's architecture notes, split out of `CLAUDE.md` (which indexes them). Read the section for an area before changing it: each rule here is a bug that was found once.

### Styling

Every colour is a token in `photon-ui`'s `theme/tokens.rs`: a light and a dark `Palette`, and
the colours that are never themed, for two different reasons. `PHOTO_LINE`, `SHADOW_INK`, and
`SCRIM` where it lies over a photo (a face's name plate, the dimming outside a crop) are drawn
onto the photo itself, which is never themed either; `SHADOW_MENU`, `SHADOW_DIALOG`, and
`SCRIM` where it dims the window behind a dialog fall on themed ground but are dark ink by
design - a shadow or a scrim reads dark against a light window too. The viewer is dark in
both themes (its glass, `GLASS`, and the dark palette for what stands on it), and its black
ground is the one colour written where it is used: a photo is judged against black whatever
the theme. The tokens' tests hold the palette to WCAG 2's contrast where each colour is used.
Icons are `icons.rs`, Lucide's path data rasterised by egui's SVG loader and tinted with a
token; a new icon is copied from `lucide-static`, whose licence is already in
`THIRD-PARTY-NOTICES.md`.

**A loop or a timer is worth asking what already-known-broken path it now drives** - the
Svelte rubber band's missed teardown was a stale rectangle until autoscroll gave it a loop,
and then it scrolled to the end of the library rewriting the selection. Anything per-frame
is worth the same question about time: multiply by the frame's own duration, or it runs
twice as fast on a 120Hz screen as on a 60Hz one.

### The window (`photon-ui`)

`crates/photon-ui` is photon's interface, in Rust on egui and wgpu (spec
`2026-10-09-photon-native-ui-design.md`), which replaced the Svelte UI and the Tauri shell in
the switch-over. **It is not launched to verify a change**:
`cargo run -p xtask -- native-shot` writes the window to `target/screenshots/native-*.png`
off screen, and those are read. Its library's photos are dated by the day their copies
are given (`FOLDERS` in `tests/screenshots.rs`): they carry no capture date, and copied
they were all of today - one month, one year, no year strip. It needs a GPU adapter (no display), so it is an ignored
test that xtask runs, not part of the gate.

Two kinds of module, and `state_modules_name_no_egui_type` holds the line between them:
*state modules* are plain Rust tested without a context (`grid/layout.rs`, `motion.rs`,
`scroll.rs`, `thumbs/loader.rs`, `textures.rs`, `tasks.rs`), and *views* draw one and turn
input into calls on it, tested in whole frames without a window (`Context::run_ui`, or
`egui_kittest` where the application itself is run, as `tests/app.rs` does). The test's
list (`STATE_MODULES` in `lib.rs`) is kept by hand: a new state module is added to it.

`eframe` is pinned to an exact version and its API moves between minors: read the pinned
version's source, never an older egui from memory. `eframe::App` is `fn ui(&mut self, ui:
&mut egui::Ui, ..)`.

**Nothing that reads SQLite runs on the UI thread.** It goes through `tasks::Latest`, which
answers only the latest question. `Engine::published` and the `GridIndex` are in memory and
are read directly - `published` when the engine says the library changed, not per frame.

**A notch of the mouse wheel is as far as the web view of the system moved it**
(`scroll::wheel_notch`, `Notch`; `wheel.rs` reads Windows' setting). A wheel reports
notches and leaves the distance to the application: egui's is forty points, which is
WebKitGTK's too, and WebView2's is a hundred at Windows' own three lines a notch - so the
native grid looked right on Linux and was two and a half times slower on Windows, which
was the first thing said by the first person to run it (2026-10-10). A hundred for
everyone was the first answer, and the second thing said was that it was still not the
web view's speed with the lines turned up (2026-10-12): **WebView2 goes by Windows'
setting, a third of a hundred pixels for each line**, a page of what is in view at "one
screen at a time" (`WHEEL_PAGESCROLL`; all but forty points of it, Chromium's rule) and
nothing at nought. winit reports whole notches whatever the setting, so photon reads it
itself (`SystemParametersInfoW`, the one call it makes of the system, through the
`windows-sys` winit already brings): when the window is made, and **again whenever the
window comes back to the front**, since the setting is changed in a window of the
system's and photon is not told. egui is told the points in every frame (a page is a page
of the window as it is then), for the frames after. Anything measured against the Tauri
photon is measured on each system: the test of this through the whole application asks
the machine it runs on, and a test may not change a machine's setting, so the rule is
tested through `App::with_wheel`, which says what the system would. On Linux and macOS
the forty stands; a Mac's wheel has not been compared.

**The grid has no scroll map.** Its position is an `f64` in the layout's coordinates and
the rows are drawn at `row.top - position`; egui's `ScrollArea` is not used for it, because
its offset is an `f32`. Several rules of the Svelte grid have no successor (`placeIn`,
`viewTop`, pages, mounting): the spec of the slice lists them so they are not ported.

**A tile is drawn wider than its size says.** The size chosen is a nominal width;
`tile_for` (`grid/layout.rs`) widens it so the columns fill the row (whole pixels, at most
`TILE_MAX`), and that drawn width is what the rows, the tiles and every hit test are
measured by. So every row's height moves with every pixel of a window or sidebar resize, and
a narrower grid is a *shorter* one: a place kept as a number names another photo afterwards.
The place is kept as a `Pin` - the row at the top by its first photo, and the share of it
scrolled past (`pin_at`, `pin_top`) - and found again in the new rows whenever the tile or
the column count changes, not only on a new size. Across a rebuilt index alone it is kept as
the number it was (below, the year strip).

**The window's motion is drawing only, and the user's switch can end it** (spec
`2026-10-17-photon-native-animations-design.md`; `animate.rs`, `grid/glide.rs`). A `Slide`
is a number moved from one value to another over `DURATION_MS` (200) by `ease_out`, for the
sidebar (`Shell`, `window_layout::areas(.., shown)`) and the info panel
(`viewer_layout::{info_room, photo_area, info_place}(.., shown)`). **The stored state flips
at the press and the slide only draws it**: a hidden sidebar is hidden, and a closing one is
drawn in a disabled child that takes no press and no key, so nothing waits for a clock;
"Rename in photon…" shows the sidebar at once (`Shell::sidebar_at_once`), since its field
needs a row in sight. The panel is clipped at its open right edge, or it would cross the
close button. **The grid's photos glide** (`Glides`, begun by `begin`) when the columns or
the nominal size change under the same `layout_gen` (`GridView::lay_out`), and after the
user's own writes: `App::write` is the one place that pushes a write, `moving` holds the
ones on their way that change a view's rows (`grid::glide::Moving`), and a grid adopted
while any is, or after one's answer was read *while a grid the engine had published was
still to be adopted*, is the user's and is handed over with `GridData::glide_from`. The
engine publishes inside the command, before the answer is pushed, so the grid lands before
the answer is read or after; an answer read once its grid was on screen, or a refusal,
used to arm the next grid adopted however late - a scan's, minutes on. A scan's grid arriving in that moment glides too, which is
accepted; any other does not. **A new write that changes a view's rows belongs in
`Write::moves_grid`**, or its photos jump. Rows the view did not lay out are no place to
glide from (`laid_from`), nor are rows the grid was not drawn in the pass before
(`drawn_pass`): laid out before the viewer, compare or the People page took its place, they
were last seen a window size ago, and the grid coming back in other columns glided from
there. A pin that moves the position under a running glide is
`Glides::repositioned`'s to keep steady. **The switch is the `animations` setting**, on by
default; off, nothing here moves and the grid's late-thumbnail fade stops too. Every test
launches `without_animations` (all but `probe.rs` and `app.rs`), and a test of a motion says
so (`launch_animated`, `opened_animated`, `over_animated`).

**A thumbnail not built yet is waited for, never blocked on** (`thumbs/loader.rs`): one
thread polls every pending `request_async`. Three rules a
review added, each with its test. A built thumbnail is handed back only under its own key
(`thumbs/source.rs`): `request_async` answers with the photo's picture as it is *now*, and
keys recur, so one stored under the key that was asked for would be drawn for another
picture. A result is not read again between being handed back and being taken (`handed_back`):
the grid lists what it wants every frame. And *not there yet* is not *cannot be made*: a
video with no poster or a photo whose drive is unplugged answers `Unavailable`, which marks
the tile (`troubled`), counts as settled, and is asked for again after `RETRY_SECS` - by a
frame the grid requests for that moment, since a still grid draws none.

**The grid's textures are held at three times what it wants** (`textures.rs`: `SLACK`,
between `LIMIT_FLOOR`, 64 MB, and `DEFAULT_LIMIT`, 256 MB), never letting go of a texture it
wants; once a library had been scrolled through, the store sat at its whole limit for the
session. **Covered** - by the viewer, compare or the People page - it keeps only what it drew
in its last frame (`Textures::rest`, called by `App::ui` in every covered frame), so it is
uncovered with its screen whole and its lead read again from the cache.

**Text goes through `text::paint_line`**, not through egui directly, wherever it can be in
another script: egui shapes text but runs no bidirectional algorithm, and draws a name that
mixes directions wrong. A right-to-left run is cut at everything that is not a letter, not
only at spaces, with what stands between two words turned round and its brackets mirrored.
The tests lay text out with egui's own fonts, which draw everything with one face: they
hold the cutting, and what a second face does is seen only in `native-shot`'s pictures or
by shaping with real faces, as the review that found the hyphen did. Not covered: a text
*field*, and cutting a right-to-left line short at its start.

**A test that reads a file as text reads CRLF on Windows.** The runner's checkout converts
line endings and `include_str!` hands them over as they are, so a match on text with a `\n`
in it finds nothing there, and nowhere else: two of this crate's tripwires failed on Windows
alone for it. Read by line (`lines()`, `trim()`), and give the reader a test that feeds it
both endings.

The stored theme and tile size are read in `App::new`, before the first frame, and not by a
task: read after it, a theme pinned against the desktop's showed the desktop's for a frame
at every launch. A screenshot sets its theme twice, stored in the library and given to the
`egui_kittest` harness, because the harness sets one of its own (dark) after the
application has been made.

**The window is four areas around the grid** (`window_layout.rs`: the top bar, the sidebar
with its splitter, the status bar, the content), drawn by `shell.rs`, a view that answers
what the user did as a list of `Action`s; `app.rs` is the one place an action changes
anything (and its child modules, `app/settings_dialog.rs`, which are `app.rs` in a file of
their own). What needs a later sub-project is drawn in place and takes no press (the
Reveals were, until the platform's): a button that will be there is there, so nothing
moves when it starts to work
(spec `2026-10-10-photon-native-sidebar-views-search-design.md`).

**A step that moves the engine's view goes through `tasks::Queue`, never `Latest`.** The
view setters rebuild the grid on the thread that calls them, and a step begun cannot be
taken back, so they are made in the order asked; `Latest` keeps one waiting question and
would put a view clicked in the place of a sort still waiting, which would then never be
made. `nav.rs` holds where the user is: `settled` is what the grid on screen shows, and
`target` is where the last step asked leads - which is what the sidebar marks, at the
click and not when a rebuild of the whole library has landed. A step to where the user is
already going is not asked (`Nav::wants`). `grid_info` reads SQLite on a cache miss
although it returns no `CmdResult`: the sidebar's counts are a `Latest`, asked again at
every change.

**What a grid shows is known from the step that built it, never read from the engine when
the grid arrives.** `Engine::view_and_arg` and `sort` move when a step *begins*, a whole
rebuild before its grid is published, so with two steps on their way the first's grid
arrives under the second's view: read then, the starred photos were drawn as All photos,
at the place All photos had been scrolled to (a review found it; the Svelte store avoids it
by awaiting each step's `grid_info` before the next). So the queue's worker reads the view
when its step is done and before the next begins, and hands it back with the version the
setter answers (`nav::Landed`); and a grid published while a step is on its way is not put
on screen until an answer vouches for it (`Nav::adopt`, `App::adopt`). **An answer vouches
for the grid it names, and for a later one only when no step is on its way**: with the
first of two steps answered and the second still being made, a grid newer than the first's
may be the second's own, and taken as the first's it stood under the wrong name for good,
the second's answer finding no new grid to put on screen. Windows' CI found that by timing,
in a test of four quick clicks that had passed a hundred runs on Linux; the rule's test is
`a_grid_newer_than_the_last_answer_waits_while_a_step_is_on_its_way`, in `nav.rs`, which
needs no timing. All of it is exact only while nothing but the queue moves the engine's
view: **a new caller of a view setter goes through the queue.**

**How the window is laid out is in `layout.json` beside `library.db`** - the sidebar's
width, whether it is hidden, which groups are open - and not in the settings table, whose
accessors are photon-core's: it is how this window is laid out on this machine. It is read before the
first frame and written off the UI thread when the user changes one of the three; the
width is stored as it was dragged and clamped where it is shown.

**The window's own size and place are written by eframe and put right by photon**
(`window.rs`; spec `2026-10-16-photon-native-platform-design.md`, the platform's):
eframe's `persistence` feature writes them, maximized and fullscreen with them, into
`window.ron` beside `library.db` as the window closes and every half minute - recording
the window as it is then, where the Tauri plugin records it only in its normal state.
Closed from the taskbar while minimized, a Windows window is 0 by 0 at -32000, and
closed maximized its normal size is the screen's; so photon follows the window's normal
size and place in every frame (`window::Seen`) and writes them over what eframe has just
written (`App::save`, called after it, through a mirror of eframe's record, `Kept`, held
to its shape by a test). eframe keeps a window on the monitors there are on Windows
alone; on X11 the window manager places one whose monitor is gone. egui's own memory is
not kept (`persist_egui_memory`): photon keeps what it remembers itself. A run asked to
be fullscreen - the gate's - keeps nothing and corrects nothing (`without_window_kept`),
and is fullscreen over what was kept (`window_builder`, which eframe applies last). A
slideshow quit in the middle reopens fullscreen, as the Tauri photon did. The window's
icon is the installers' PNG (`icons/photon-256.png`).

**The sidebar is one list, drawn a window at a time** (`sidebar/list.rs`, a state module,
and `sidebar/view.rs`). Every row is an `Entry` with what it is (`What`, its identity from
frame to frame and what a click answers), and a height; `Stack` adds them up and the view
draws the entries its position takes in, as the grid draws rows. The position is the
view's own number, kept by `Shell`, so a hidden sidebar is not drawn at all and comes back
where it was. Building the entries is an allocation each, so `List::follow` builds them
again only when a key of what they were built from has moved - the counts, where the user
is going, the folds, the sort, the grid's layout generation - and the two sources too large
to compare, the collections and the folder list, live in `Held`, which has no way to
replace either but a setter that moves its number. A new thing the list is built from goes
into `Sources` and into its key; left out of the key, the list goes stale and no test of
the others will say so. The stack is *not* cached: it is a sum over the entries, and a
cache keyed by list, width and scale was removed when no test could tell it was there.
A press that lands a few points beside a row is the nearest row's: egui gives it to the
nearest button in reach, so the room between two rows is not dead as it is in a browser.

**A folder is gone to in the grid that holds it** (`Nav::jump`, `Nav::home_of_folders`,
`GridView::go_to`). A click on a folder from an excursion asks for All photos and keeps the
jump with the number of that step; `Nav::adopt` hands it out with that step's grid and with
no other, and the application looks the folder up in the index it has just put on screen.
Hidden is left as it is. The offset is given to the grid in the same frame as the index it
was looked up in, and the grid takes it once its rows are laid out. `LastFolder` (also
`nav.rs`) is the folder photon remembers: read once before the first frame and kept in
memory, since the application is its only writer - which is why the way back from an
excursion reads nothing, where the Svelte UI has to read the setting before the switch can
overwrite it. The launch goes back to it in the frame its first photos are on screen and
before they are drawn, so there is no frame with the grid at its top for `at_top` to
remember; a guard for that frame was removed as doing nothing. A run of the probe keeps no
folder at all (`LastFolder::off`): its programme starts from the top.

**The mark of the folder at the top of the grid is a frame behind the grid**, the sidebar
being drawn before the grid has taken its place, and the application asks for the frame
that makes it up (`top_folder != marked`). Nearly always one would follow by itself: the
grid moves on input, on a task's answer or in a scroll, each an immediate request, and egui
draws two frames for each of those - which is why taking the request out changed nothing
any test could see, and it was taken out. A review put it back with the case that needs
it: eframe draws one frame for a window resize that nothing asked egui for, a grid at the
end of a library is held to its new end in it, and the mark was a folder behind for the
hundred and fifty milliseconds until the grid's own report of what is in view
(`the_mark_is_brought_up_to_a_grid_that_moved_in_a_frame_nobody_asked_for`). "Each way a
thing can move asks for a frame" has to be checked against eframe's own frames too.

**The list is held by an entry when its entries change** (`Kept`, in `sidebar/view.rs`):
the row under the pointer, else the marked folder's while it is in sight, else the first in
view, by what it is and not by its index, and the list is put where that entry is where it
was. The albums are read a moment after the folders, and at launch the list has by then
followed the grid to the remembered folder, flush at the bottom edge: their rows went in
above and pushed it out of sight, with nothing to bring it back, since the list follows
once for each folder. A fixture's lists are read inside a frame, so no test of the whole
application saw it; a review did, with a sleep in the task.

**A folder asked for is kept as the folder until the frame that draws it** (`App::going`,
`take_folder`), not as its offset: a click is acted on after its frame is drawn, and the
next frame may draw a scan's grid with photos more above the folder. Everything that sets
a place in the grid from outside is kept that way - a folder in `going`, the photo the
viewer closed on in `showing` (`take_photo`) - and taken after `adopt` and before the
grid is shown. The viewer's close handed the grid an offset at first, and a review's test
put a grid of three hundred photos more between the Escape and the next frame. A row half scrolled out is drawn and pressed through a child clipped to the
sidebar: `Icon::paint` paints through the `Ui` it is given, and through the shell's the
icon of such a row stood on the top bar.

**The collections are not asked for at launch**: the first grid the engine publishes is a
data change, built or failed, and that is what reads them. A row tells AccessKit whether
it is where the user is (`WidgetInfo::selected`), which is how a test of the whole
application sees the fill; a test that clicks two rows in a row draws a frame between
them, because the second is clicked where the last frame drew it and a fold moves it. And
a test that waits for something *not* to happen settles after the clock has passed the
moment it was due, not only before: `frames_in` jumps the clock without waiting for the
other threads, and a search sent in it has not been answered when it returns.

**The search box is three modules.** `search_box.rs` (state) holds the text and the send
that typing has made due, on a clock that is a number; `search_help.rs` (state) is the
grammar's list, held to `Query::terms` by its tests; `search_bar.rs` draws egui's own text field, the two buttons and the
panel. A typed search is a `Step::Search` on the views' queue, taking the place of a search
still waiting there (`Step::is_search`) and of nothing else. **Whatever switches the view
tells the box first** (`App::go`: `cancel` before anything, `leave` as the switch is
asked for, `search` for a row that is a search), or a search half typed lands after the
switch and replaces the grid that was asked for; a folder click and All photos cancel too.
The box's text is written from this side alone: it is never read back from the engine.
Things about egui's field that cost a test, a picture or a review each. Escape is claimed
for the field (`TextEdit::event_filter`): left to egui it takes the focus away at the start
of the frame, before anything is drawn, the box cannot tell it had had the keys, and the
grid hears the key too. The field stands at the top of the room it is given, so it is given
a room one line tall, and a press on the rest of the field is answered by a second,
unfocusable widget under it. An `Area` starts at egui's default size, four hundred points
tall, and keeps the size it was first given: the help panel says how tall it may be every
frame. **The `/` that opens the box is let go of by whatever key comes up, and by the
window being left** - not by a release named Slash: where `/` is Shift+7 the key that comes
up after Shift is `7`, the slash was held for good, and every `/` typed in the box was
swallowed, `f/1.8` among them. And **the grid's keys are a text field's while it has the
focus, its scrollbar is not** (`grid/view.rs`): the early return that made the grid deaf
stood above the scrollbar too, and with the caret in the box the thumb did nothing - egui
gives up a field's focus on a click elsewhere, not on a press that is held. A press on the
bookmark is a write, made on a queue of writes (`App::writes`) and followed by a read of
the lists, since the engine announces no data change for a search saved; until they are
read the search does not look saved, so the box itself knows what is being saved
(`SearchBox::save`, `can_save`) and a second press saves nothing. **A window with the
caret in a text field is not still**: the caret blinks, about six frames a second, as in
any application. `egui_kittest` switches the blink off, so no test of the whole
application sees it, and `a_window_with_a_search_shown_draws_nothing` leaves the box with
Enter first to say what it does hold: a search shown, with the keys back in the grid.

**The controls at the right of the top bar hold nothing of their own.** `controls.rs`
(state) is what they offer and what a choice makes of the sort, `controls_bar.rs` draws
them, and the sort drawn is `Nav::sort_target`: the sort the last step asked leads to, as
`target` is the place. A control answers that sort with one field of it replaced, and a
change of sort is a `Step::Sort` on the views' queue, where it gives way to nothing and
nothing gives way to it - so a second change made before the first has landed builds on
the first, and a refused one puts the controls back. A sort moves no view and clears no
query: `App::go` leaves the search box alone for it. **A sort's error does not always mean
nothing moved**: `Engine::set_sort` rebuilds and publishes, then stores, so a sort it
could not store is on screen all the same, and the queue's worker answers that as a step
made, with the engine's words for a message - taken as refused, the photos were in one
order and the controls said the other for the rest of the session. And anything asked
*behind* a sort on its way asks `sort_target`, not the sort on screen: All photos did
not, and came back to the remembered folder in an order that has no folders. The size is not a step: it is
applied at the press and stored on the queue of writes, and a size the library would not
store is kept for the session, as the Svelte store keeps it.

**The dropdown is `select.rs` and `select_view.rs`**, the Svelte `createSelect` and
`Select.svelte`. The control has the keyboard and the list never does. What egui does
around a focused widget decided most of the view, each learnt from a test that failed or
a probe that passed. A click does not give a widget the keyboard: the control asks for it
at a press. Enter and Space on a focused widget are a click to egui, and so is the press
a screen reader makes: the keys are read as keys, and a click in a frame with Enter or
Space in it is not a press. **Tab is read as the control loses the focus**
(`lost_focus`): egui has moved the focus on by the time the control is drawn, and read
only while it had the keyboard, Tab closed the list without taking the option it was on.
The up and down arrows are claimed (`set_focus_lock_filter`), or egui walks the focus to
a neighbouring widget with them; while the list is open so are the sideways ones, which
do nothing in it and otherwise closed it by walking the focus off, and Escape, or egui
drops the focus before the control sees the key. A key the list used is taken out of the
frame's input - a letter typed with the key that typed it, as well as an arrow - because
the grid is drawn after the bar and would scroll by the same arrow, and will one day hide
a photo by the same letter; which is why the controls are drawn after the search box,
which has read its own keys by then. **A pointer that closes the list gives the keyboard
up**: by choosing from it, by pressing the control again, by pressing elsewhere. egui
lets go of the focus only at a click on something else, never at a wheel turned or a
scrollbar dragged, so a sort chosen with the mouse left End opening the list from
wherever the grid had been scrolled to, and Enter after it sorting by size (a review
found it). A screen reader's press is no pointer and leaves the keyboard where it is. A
dimmed control is no stop of the Tab key, where the Svelte one is focusable and inert.
**The list closes at a press outside it, by the view's own rule**: egui gives up the focus
at a *click* elsewhere, which is the button let go of, and never at a press that becomes
a drag - with only the "no keyboard, no list" rule, a list stayed open over a scrollbar
being dragged. A click on the list is a click elsewhere to egui, so the list is spared for the frame of
that click and for nothing else: a pointer merely resting on it does not hold it open
when Ctrl+F takes the keyboard. The pointer makes a row the active one when it moves
(`pointer.delta()`, not `is_moving()`, which is a smoothed speed that one step after a
rest does not move), not while it rests where the list opened. And the list is as wide as its longest option needs in a row, where the mark's
room is more than the chevron's: at the control's own width it printed "Date modi…",
which a picture showed and no test had (`Galley::elided` is how a test sees a text cut
short; `Galley::text` is the whole text either way).

**The year strip is asked for before the rows are laid out, because it decides how wide
they are** (`GridView::strip`): `shows_timeline` wants the number of year marks, and
`timeline::year_runs` counts them from the sections alone, held to `year_marks` by a test
that runs both. The count is kept for the layout generation it was made in. The strip
stands at the grid's right edge with the scrollbar left of it, as in the Svelte grid, and
the photos have what is left. When the strip comes or goes with a new index the tile
changes with it, which is the pin's own trigger - so the place carried over is the
photo's *offset*, from one index into another: right while nothing arrived before that
photo, and off by what did otherwise, as the kept number is across any new index (a
review's test with a folder of a newer year; the grid does not follow a photo by id). A press on
the strip is the primary button's and follows the pointer on or off the strip
(`is_pointer_button_down_on`); the year under the pointer is named over the photos, so it
is painted last and through the grid's own painter. It is no stop of the Tab key
(`Sense::CLICK | Sense::DRAG`). A guard against a strip of no height was removed: nothing
divides by it that a test or a panic could tell.

**The status bar's lines are `scans.rs` and `status.rs`**, what the Svelte store holds of
the engine's reports and `status.ts`. The application gives `Scans` every report and is
told what to do next (`Asks`): read the folder list again, read the published grid
again. The Svelte store's rules are there with their reasons and tests - the list read
once for a scan in a folder it does not hold, a done from a drive that is only polled
being no scan ending, a notice only for a folder still watched. **One is not: what a scan
is measured against is said by the scan** (`ScanProgress::known`, on every report but a
full scan's first, and let through the engine's quarter-second throttle the first time it
is said). The Svelte store counts the folder's photos when it hears a scan's first
report and takes out what that report had added; this window hears the report when a
*frame* takes it, and a first scan writes five hundred photos every few hundred
milliseconds - a review's test, with no frame drawn for 400 ms, read "3,000 of ~500
files". Only the scan knows what its folder held before it. **The bar of a first scan
has no clock** (`Bar::Unknown`): it steps once for each report heard of the scan, because
a bar that moved by itself would draw a frame sixty times a second for as long as the scan
ran - by the reports and not by the files they count, which arrive in any number. The
bar is one row: the count is drawn first and the lines have what is left, and a line with
no room for its words is left out with its bar.

**A number is shown beside the thing it was read with, or not at all.** Three times in one
review the window paired a number with a state it was not read for: the count above; the
hidden photos, counted a round trip after the grid that lost them was on screen, so every
photo hidden at once said "photon has found no photos" for that long (`App::counted` is
dropped when a grid that held photos comes back empty); and a folder's `online`, read
with the folder list a frame after the scan that changed it said it was done, so a drive
just back was one photon "cannot reach" (the scan's `FolderStatus` is written into the
list on hand at once). The Svelte UI gets the first two for nothing, from one answer that
holds both numbers.

**The launch is the one thing an empty library has to wait for that the Svelte UI never
met** (`scans::Launch`, `Engine::startup_scans_started`). The engine publishes its first
grid, *then* watches the Pictures folder and starts its scans; a webview is nowhere near
loaded by then, and this window is drawing. For those moments it has an empty grid, a
folder list with nothing in it and a count of hidden photos - everything it would need to
say "Choose a folder" over a library a moment from being scanned, and "No folders yet."
beside it. So nothing is known of the folders until the list *read after* the engine says
its first scans are started, and each folder of that list counts as being scanned until
its scan has been heard from, or the folder is no longer watched. A scan reports at least
its end unless it dies: one that panics says nothing more, the engine does not say so, and
an empty library waiting for it goes on looking. The engine is asked every fifty
milliseconds until it says its scans are started, since nothing reports the moment.
**The seam for all of it is the library's write lock**, taken from a second `rusqlite`
connection in a test: held, the engine's next write waits (a scan stands at its first
write with its reports made, the startup thread before it has watched the Pictures
folder); held past the five seconds a write waits, the write *fails* - which is the one
launch that sends no event at all, and the test of the fifty-millisecond asking and of
the list asked for again (a review found that one). A test that waits to *see* something
during a scan is held that way and never raced: on macOS' CI a scan of eight hundred files
was over before the window had drawn its second frame. `Driver::settle` spins while the
launch is being asked about - a frame is always due within fifty milliseconds - so a test
that holds the launch waits in real time instead. **What the Svelte UI's
`scanning_folders` and `noteRunning` are for does not exist here**: the channel is made
before the engine is opened, so no report is sent before someone listens.

**The empty library's panel is `empty.rs` and `empty_panel.rs`**, and the view of it in
All photos and Recent alone; every other view has its one line. "Add folder…" opens
the system's folder picker and watches the folder chosen (`App::pick_folder`, below),
"Watched folders…" Settings at its folders; and "Or
drop a folder onto this window." stands under them only where a drop can arrive
(`LibraryFacts::drops`, below): a line that offered one where none arrives would be
false, not inactive. The block is centred, so a longer sentence
pushes its buttons down, as in the Svelte grid; its sentence is wrapped by egui and does
not go through `text::paint_line`, so a folder's path in another script is drawn in the
order it is written. The sidebar's notes - the empty groups', and "No folders yet." -
tell AccessKit their words, which is how a test of the whole application reads them.
"No folders yet." stands at the folders' own inset with air over it, over the button to
Settings' folders: at a note's indent it read as the note of the group above it.

