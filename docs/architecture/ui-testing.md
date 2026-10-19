# The window: tests, installers, locale, probe, build notes

Part of photon's architecture notes, split out of `CLAUDE.md` (which indexes them). Read the section for an area before changing it: each rule here is a bug that was found once.

**A picture test makes its GPU device alone** (`tests/screenshots.rs`, `DEVICES`): every
picture is drawn on a thread of its own, and with two dozen devices made in the same
moment `native-shot` crashed inside the Vulkan loader about once in four runs
(`vkEnumerateInstanceExtensionProperties`, read from the core dump). Making them one at
a time ended it; the pictures are still drawn side by side.

**A star is tested in a library of real files** (`tests/menu.rs`, `pictures`,
`over_files`): the folder's `.picasa.ini` is read back, and every write goes into a
temporary directory the test made. A fixture's folders are on a drive that is not there,
so a star there is the engine refusing - which is the test of a refusal's message. A
person to take photos off is made through the library's own calls before the launch
(`write_face_batch` for every photo, so the launch's face pass finds nothing to detect,
then `name_items`).

**Tests of the whole application drive it through `tests/common`'s `Driver`**: `until`
draws the frames that are asked for, `settle` waits in real time for the other threads,
`frames_in` counts what a stretch of the clock draws. **A sidebar row is clicked once it
is drawn** (`common::click`, `Driver::seen`): Hidden and Starred are there only while
the library holds something of their kind, which the counts say a moment after the
grid, and a test that clicked "Hidden" straight after the grid found no such row on
Windows' CI. **A wait asks for what is said,
never for how many messages stand**: when nothing has answered for a quarter of a second
of real time `until` lets the clock go on to the next frame asked for, which is the end
of a message's time, and a test that waited for "two messages" behind a slow write then
waited for ever - on Windows' CI once, after passing everywhere a dozen times. A row is clicked by its name
(`harness.get_by_label("Starred").click()`, which is the pointer's click at the node's
place; a screen reader's is `click_accesskit`), which is why every row and button tells
AccessKit what it is (`widget_info`) - and since the platform's sub-project a screen reader
hears it too (eframe's `accesskit`). Two things about egui's input that a frame test meets: egui works out for
itself whether a key going down is a repeat, whatever the event says, and it shows a new
`Area` - a toast - only in the frame after the one it measures it in.

Three smaller things the same review found. The list of messages a frame draws comes from
`Toasts::at`, which lets go of those whose time is over first: drawn first and let go of
after, the frame asked for at a message's end drew it once more, and on a still window
that picture stayed. `text::paint_line` gives its text its own colour: the application sets
one colour for all of egui's text, egui puts that in place of a painter's fallback, and
every dimmed line - a header's count, the line of an empty view - had come out in the full
colour since sub-project 1. And the splitter is lit under the pointer, while it is held and
when the keyboard brought the focus to it, not while it merely still has the focus a
press gave it (`Shell::pointer_focus`): egui has no `:focus-visible` of its own.

**The installers are built by `xtask package`, in `release.yml`** (spec `2026-10-16-photon-native-platform-design.md`,
decision 8): a `.deb` (`dpkg-deb`, its `Depends` read off the binary by `dpkg-shlibdeps`),
an AppImage (`linuxdeploy`, which carries the libraries the binary links - GStreamer's
among them, never its plugins - moved off the runpath to `usr/fallback`: `AppRun` uses the
host's GStreamer wherever the host has one, and the carried one, pointed at the host's
plugins, only where it has none - the carried 1.20 refused Arch's 1.28 plugins and every
video said GStreamer was missing), a `.dmg` for each
Mac (an ad hoc signed `photon.app`, `hdiutil`), and an MSI (WiX 3 over
`crates/photon-ui/packaging/windows/`, photon's own `main.wxs`: Tauri's template with its
placeholders out, the Tauri MSI's upgrade code as a literal so that the native one replaces
it, the opt-in desktop shortcut kept). The program is installed as `photon` under the
Tauri photon's identifier. The text of every file an installer carries is a pure function
of `package.rs` and tested as text; the tools run in the workflow alone, which reads what
they made: the `.deb`'s links (no GTK, nothing unresolved), the AppImage's desktop entry,
no plugins in it and no GStreamer on its runpath, the app's signature, the MSI's tables.
**Media Foundation is delay-loaded** (`build.rs`, `/DELAYLOAD` for `mfplat.dll` and
`mfreadwrite.dll`, held by the workflow's `dumpbin /imports`) and loaded by `video::mf`
before any call into it (`installed`), so a Windows "N" edition without the Media Feature
Pack starts and says it cannot play videos - a delay-load that fails is an exception Rust
cannot catch. The MSI has the Tauri MSI's upgrade code and allows the same version: over the
Tauri photon of its version or a lower one it removes the Tauri program (the library is left
alone), and over a higher one it is refused - the workflow's summary says so. The installers
are built from paths made whole first (`std::path::absolute`): dpkg-shlibdeps, linuxdeploy
and light each run in a folder of their own, and `--out dist` was read from there.

**What the switch-over's audit of the Svelte checklist ported** (spec
`2026-10-16-photon-native-switch-over-design.md`, each with its test). Closing the viewer
on the photo the keys were on keeps the selection it stands in (`close_viewer`'s
`on_lead`; Shift+End, Enter, Escape lost the range). A tile's marks have a halo of
`SHADOW_INK` (`tile::halo`); the tiles of a folder whose watched folder is offline are
drawn at 0.4 (`offline_folders`, `GridData::offline`); a thumbnail that arrives later than
two frames after the loader was asked for it, onto a still grid, fades in over 120 ms
(`Textures::asking`, `arrived`, `upload`'s `fade`, `shown`, `INSTANT_SECS`): timed to when
the loader hands it over and not to its upload, which is rationed a few a frame, and
counted from when the loader was asked and not from when a deferred scroll first passed
over it - either way a screenful of cached thumbnails counted as late and faded in after
every fast scroll (a review). One arriving while the grid moves does not fade. A mark's
halo is one enlarged paint of the icon in the shadow's ink, not one a direction: an
icon's paint goes through egui's loaders, and eight cost a marked grid three times its
painting. A playing video is
fitted above its strip (`fit_in` in `ViewerView::show`). The search box has the web
view's own cross (`CLEAR_LABEL`), and its help closes when the keyboard leaves the box and
its buttons and its own terms (`had_keys`, `term_keys` - not egui's `read_response`,
which takes the keyboard off a focused field whose click lands elsewhere, and so took it
off the People page's name box at the click on a person offered under it). An open keyword's view follows the keyword renamed in Settings
(`Nav::renamed_tag`, from the stored name `commands::rename_tag` now answers): the engine
moves its own argument outside the views' queue. The info panel says a whole path under
the pointer. The sidebar's rows, the empty library's buttons and the top bar's size
buttons draw the keyboard's ring (`settings_view::ring`); the sidebar draws a row past each
edge, cut away and only a stop of the Tab key (`Sense::FOCUSABLE`, no press - a press
just under the list was handed to it as a near miss - and no name field, which drawn
there was sent in every frame), so that Tab walks on below the edge, and brings the row the keyboard has
just reached into sight (`keyed`); a name left from the keyboard gives the keyboard back to
its row (`back_to`), unless the rename was begun on a header in the grid
(`rename_from_grid`, `menu_on_header`). What was left, and why, is the spec's list.

**Dates and counts are written in the system's locale** (`locale.rs`, the user's answer,
over English only): a period's header, a folder's month, the viewer's caption and the info
panel's dates, "On this day" and every count with thousands (`grid::labels::grouped`), as
the Svelte photon's `Intl` wrote them. ICU4X's formatters over its compiled data
(`icu_datetime`, `icu_decimal`, pure Rust, new to the lock but on the `icu_provider` and
`icu_locale_core` it had already; `icu_provider`'s `sync` feature, so the formatters can be
one process-wide value). The locale is read once, by `main` and not by `App::new`, which
every test calls (`sys-locale`, `Locale::system`, a Linux `de_DE.UTF-8` read as `de-DE`),
and held for the process (`locale::set`, `current`); English until set, which is what
every test reads - and a unit test's other locale is its own thread's
(`for_this_thread`), so that under `cargo test`, one process, it is no other test's. A
locale ICU4X has no words for is none, and English is written: its own fallback is the
root's "2026 M10 4", where a browser falls back to its default. A month and a day are of
the real day, never a month's first or a stand-in year: a locale's calendar may be another
(the Persian's months turn round the 21st). Dates in the info panel go through
`paint_line`, which turns a right-to-left run round (Hebrew, Arabic). ICU4X's narrow
no-break spaces are written as plain ones (`plain`): the faces photon draws with are the
system's, and not each has that glyph. Folder names keep `natural_cmp`, the grid's own order
(the spec says why). The words photon writes itself stay English.

**`photon --probe` runs a scroll programme over a library and reports how each frame
went** (`probe.rs`, a state module), over `xtask fixture-library`'s 300,000 rows on a drive
recorded as unplugged, every thumbnail cached, so the engine has nothing to do at launch.
Until the switch-over `xtask grid-gate` ran it against the Svelte grid given the same
programme, which is how the window's grid was shown to keep up with the web view's; the
gate went with the Svelte grid. It launches the application, so it is run by a person, or on
their word at the time. The fixture's builder says whether a fixture is finished and its own
(`fixture --check`, `fixture::BUILDER`): **move `BUILDER` with any change to what a fixture
is.**

**A test that counts frames drives the application as an event loop does**
(`the_programme_runs_over_a_fixture_to_its_report`): a frame only when egui's repaint
callback asked for one, on a clock that jumps over the time nothing did. Stepping
`egui_kittest` draws a frame at every step, asked for or not, and hid what the probe's first
count of idle frames got wrong: egui draws a second frame after every frame asked for at
once, a frame asked for after a delay comes one frame early, a scroll leaves two frames of
its own behind (`SCROLL_SETTLE_MS`, `VISIBLE_DEBOUNCE_MS`), and a nudged mouse is three
frames, not one (`ECHO_MS`). Each would have read as a busy grid that was perfectly still.
Anything that asserts a still interface draws nothing is tested through that loop.

**glibc's mmap threshold is fixed at launch** (`allocator.rs`, `mallopt`, first thing in
`main`). By default it slides up to each large block freed, and every photo buffer after the
first few came out of a thread's arena, which keeps what is freed: an import of 48 photos of
12 MP left 165 MB resident over 4 MB in use, and `malloc_trim` returned none of it. Fixed at
1 MB, it left 4 MB. The figure Settings shows is that resident memory.

**Two checkouts do not share a `CARGO_TARGET_DIR`.** Cargo names a workspace member's
build by its place in the workspace, not by where the checkout is, and decides whether it
is fresh from the sources of whichever checkout built it last: a second checkout then runs
the first one's binary without a word. It happened twice while the grid gate was built (a
reviewer's copy, and the gate's own worktree running this tree's xtask).

