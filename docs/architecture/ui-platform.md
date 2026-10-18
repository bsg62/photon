# The window: drops, export, folder picker, clipboard, reveal, GPU

Part of photon's architecture notes, split out of `CLAUDE.md` (which indexes them). Read the section for an area before changing it: each rule here is a bug that was found once.

**A folder dropped on the window is watched** (`folder_drop.rs`, `createFolderDrop`;
`drop_card.rs` for the card). The application takes what the system says is held over
the window and dropped on it out of each frame's input before egui reads it
(`App::raw_input_hook`; `RawInput::hovered_files`, `dropped_files`: the system's own
events, a file each) and hands a drop's paths to the engine as one write
(`Write::Watch`), in order, each offered after the one before has been answered - two
folders of one drop can overlap, and the engine can refuse the second only once it has
finished the first. The engine is what knows a folder from a file and a folder already
watched; the drop is reported once (`Watched::said`, `refusal`: the first refusal with a
count of the rest, so that forty photos dropped are not forty messages). **Whether a drop
can arrive at all is asked of the window, once** (`folder_drop::can_arrive`, from the
window's handle in `App::new`): winit 0.30 has dropped files on X11, Windows and macOS
and none on Wayland, so a Wayland window offers none - no card, no line in the empty
library's panel - and a test, which has no window, feeds the drop itself
(`common::hold_over`, `drop_on`; `App::without_drops` for the window that takes none). A
folder just watched is in the list on hand at once and counts as being scanned until its
scan has been heard from (`Scans::added`, into the launch's own set): the list read
again arrives a moment later, and an empty library must not say meanwhile that no folder
is watched. That call has its tests in `scans.rs` and none through the application: the
answer that says a folder is watched and its scan's first report arrive in either order,
with no seam to hold one behind the other. One folder twice in a drop - by its path, and
by a link to it - is one folder, by its id. **Under the export's dialog the first path
dropped is its destination, and nothing is watched** (the first path, not the first
folder: a photo dropped before a folder is said to be no folder, as a file typed there
is); under anything else that is open a drop is the window's, as ever. The card is drawn
over everything, a dialog too (`Order::Tooltip`), takes no press, and tells AccessKit its
words, which is how a test of the whole application sees it.

**egui is not told what is held over the window.** For as long as it is, it asks for the
next frame at once (`InputState::wants_repaint_after`): a folder held still over the
window drew a frame at every refresh, and the card's own frame test, which draws the
card without the input that puts it up, could not see it - a review counted the frames.
The system's reports each draw a frame without that (`egui-winit` asks for one at each),
which is all the card needs. `egui_kittest` does not call `raw_input_hook`, so the tests'
`Driver` does before every frame (`common::hand_over`) and keeps what is held from frame
to frame, as `egui-winit` does (`Driver::held`). **And the system does not always say
that a drag has left**: winit on X11 can report a file held after it has reported the
drag gone, when the two cross - read in its source, not reproduced - and what it goes on
calling held is then held for good. So a pointer that moves or is pressed in the window
ends the drag, and so does a drop, whatever the system still says
(`FolderDrop::frame`, its `over`); the next drag is one more path than was left over.
That a window hears nothing of the pointer while another program's drag is over it is
how a drag is made on X11 and Windows and is assumed of macOS: were it not so there, the
card would not show and the drop would still be taken.

**The export is a third dialog and a thread of its own** (`export_dialog.rs`,
`createExportDialog`; `export_view.rs` for its body; `App::exports`). Its photos are
taken as it opens, as the picker's are. **The destination is a path in a field**, typed or
put there by the system's picker ("Choose…", below): asked about as it changes, off the UI thread, by a task that answers
only the latest question (`App::checking`, `check_dest`), and the answer counts only for
the text the field still holds (`ExportDialog::checked`). It passes when it is the whole
path of a folder that is there (`typed_path`: a `~` at its start is the home folder, and
a path that is not absolute is refused - it would be read from wherever photon was
started) and the engine will write into it; a file passes the engine's own check, which
asks only where a path lies, so the task asks `is_dir` first. **The text as it stands is
asked first, and without the spaces at its ends only when no folder is at that very
path**: a folder's name can end in a space, and a dropped folder's path is put in the
field as the system gave it - read trimmed, "out " dropped passed as the folder "out"
beside it, and the copies were written into a folder nobody had named. What is said of a path
that does not pass stands under the field, the engine's words for a folder inside the
library among it, **and stands there until the next answer** (`ExportDialog::problem`):
a path is asked about at every letter, and with the line taken away at each letter and
put back at each answer it came and went as a path was typed, the dialog in the middle
of the window moving up and down with it (seen in a picture whose click missed a button
that had just moved, and in no test until then). The room of one line is kept under the
field whether or not anything is said there, so the reasons of one line move nothing;
the engine's two lines make the dialog taller. **And every dialog is placed by the
height it has in the frame it is drawn in** (`dialog_view.rs`: an `Area` of its own
with `Modal`'s settings, since `Modal`'s is anchored, and an anchored area is placed by
the size it had the frame before): in the frame the export's dialog changed height its
top stayed where it had been, a press on "Export" where the button had stood was a
press beside the dialog - which closes it - and it was put in the middle a frame later.
"Apply edits" is read when the dialog opens (ticked until then, and
not over a tick made meanwhile), stored on the queue of writes as it is ticked, and put
back if the library will not store it. The size is five buttons in a row where the
Svelte dialog has a dropdown, and is never remembered. Enter in the field exports once a
destination has passed; what was typed in the frame of that Enter is asked about first.
**The dialog closes before the export runs, and the export is not on the queue of
writes**: it takes minutes, and a star pressed meanwhile must not wait for it. One at a
time (`App::exporting`): "Export…" takes no press while one runs, and the status bar's
line is the engine's own reports - the first when a photo is done, so nothing but the
dimmed item says that an export stands at its first photo. The window does not wait for an export when it closes,
as the Tauri photon does not. Nothing here writes a file itself: the copies are the
engine's (`Engine::export_items`, which refuses a destination inside a watched folder
and creates every file with `create_new`), and a test's destination is a folder under
its own temporary directory. A test holds an export for as long as it needs by holding
the engine's `RENDERING` lock, which a turned photo's copy waits for.

**A folder is chosen in the system's own picker** (`platform.rs`, `app/pick.rs`; spec
`2026-10-16-photon-native-platform-design.md`, decision 2): on Windows and macOS rfd's,
the crate Tauri's dialog plugin is built on; on Linux the XDG desktop portal over D-Bus
through ashpd, the client rfd uses there (pure Rust, no GTK, on `async-std` - with
tokio, zbus panics with no runtime running), and where the portal does not answer,
`zenity`. **rfd is not built on Linux**: there it must have GTK or the portal, and its
build script refuses both. Going to the portal itself also tells a picker that could not be opened from a
cancel, which rfd answers alike: no portal and no `zenity` is said (`NO_PICKER`), a
cancel is not. "Choose…" beside the export's destination and beside Settings' folder
field opens it, and the folder chosen goes into the field and is asked about as a typed
path is; **"Add folder…" - Settings' and the empty library's - opens it and watches the
folder chosen** (`PickFor::Watch`, the drop's `Write::Watch`), as the Tauri photon's
`addFolderFromPicker` does, whatever has opened over the window by the time it answers -
but a picker open for the export is not taken for it ("Export copies to…" would have
watched the destination chosen), and where none can open Settings' field is opened only
over nothing or over Settings, never in place of another dialog.
It opened Settings' field first and the picker over it, steps the Tauri photon does not
have, which the first person to run the window found. Where no picker opens, that is said
and Settings' field is opened - the way left. **It runs on a thread of its own, one at a time, and its answer is for the
field that last asked for it** (`App::picking`, by the number of the dialog's opening,
`App::dialog_openings`): a press while one is open - a picker tied to no window, as a
portal's on Wayland and Windows' with no owner are, can be behind photon's - opens no
second one and takes the open one for that field; a folder chosen for a dialog closed
since goes nowhere. "Choose…" is never dimmed for it: a dimmed button senses no press,
and egui takes the keyboard off what senses none. The portal is given ten seconds to open
its picker (`PORTAL_PATIENCE`) and the user all the time they want; a place that is no
file of this computer's (`smb://`) goes into the field as it is, which says it is no
whole path. Where photon has no window to say something in - the library that could not
be opened - it says it in the system's message box (`platform::show_error`: `MessageBoxW`
on Windows, since rfd's is a task dialog under the workspace's unified features and
needs Common Controls 6, which photon has no manifest for; rfd's on macOS; `zenity`
on Linux where that is installed), on the standard error and in the log, in the Tauri
photon's words (`platform::startup_failed`).
**The system is one trait, `Platform`, and a test stands in for it** (`App::with_platform`;
`common::Picker`, which answers and can be held): every test builder that can press
"Choose…" or the empty library's "Add folder…" launches with one (`no_platform`), or the
press opens a real picker on whatever machine runs the test.

**Copy photo puts the picture as shown on the clipboard** (`app/copy.rs`; spec decision
4): the grid's Ctrl/Cmd+C with exactly one photo selected and no video (the Svelte grid's
rule - not the lead of a wider selection), the tile menu's "Copy photo", and the viewer's
Ctrl/Cmd+C over a photo the library has said is one. **Ctrl/Cmd+C reaches egui as
`Event::Copy` and not as a key** (egui-winit turns it into that and nothing else), so
that is what the grid and the viewer read - not while a keyword is typed, whose field
copies its own text, and not with Shift or Alt, which egui makes a copy of too (its
repeat cannot be told from the event: a held key copies again as each copy ends). The
shortcut list's tripwire counts it by the `C` the same groups name, so it cannot see the
copy's handling go. The engine makes the picture (`commands::copy_picture`, the edit applied, at most
2,560 pixels a side, under `RENDERING`) and the platform puts it on the clipboard, on a
thread of its own; **one copy at a time, and the last photo asked for is the one on the
clipboard**: a second press for the photo on its way is dropped, as the Svelte store drops
it, and one for another photo goes after it (`next_copy`) - dropped, "Photo copied" was
the first photo's and that was what was pasted. "Photo copied" when it is done, the
clipboard's own words when it is not. **The clipboard is one arboard `Clipboard`, kept for the session by a
thread of its own** (`platform::System`): on Linux a copy lives only as long as the
clipboard that made it, and not the window's own (`Context::copy_image`), since arboard's
X11 side makes a PNG of the picture as it is put there. arboard's `wayland-data-control`
is on - Rust, the Wayland library loaded at run time as winit loads it. A click on the
viewer's caption copies the file name through egui's own clipboard (`copy_text`), and the
caption reads "Copied" for `COPIED_MS`, of that photo alone - **with the bar as wide as the
line it stands for** (`ViewerData::caption_said`): laid out for the shorter word, the bar
shrank and the second click of a double click on the name turned the photo. On X11 with
no clipboard manager a copied photo is gone once photon quits, as any X11 program's copy
is. A test's clipboard is the
stand-in (`common::Picker`'s `copied`, `refused`), and a copy is held behind a render by
holding `RENDERING`.

**Reveal and Open ask the system, off the window's thread** (`app/open.rs`,
`platform.rs`; spec decision 5): every "Reveal in file manager" - the tile menu's and
Ctrl/Cmd+Shift+R for one photo selected, the viewer's menu, a folder's menu, a watched
folder's row and About's library in Settings - every "Open in default app", and the info
panel's "Open in OpenStreetMap" (the engine's `item_map_url`). Each asking is a thread of
its own (`Opening`): the path is the library's, read off the window's thread, and a file
on a share that has stopped answering or a file manager starting can take seconds; what
goes wrong is said, in the system's words. **The path is asked about before the system
is** (`there`, as Tauri's opener asks it): the file manager on Linux answers for any path
and `xdg-open` is let go of before it finds out, so a photo on an unplugged drive was
revealed to nobody with nothing said. Reveal is the shell's `SHOpenFolderAndSelectItems`
on Windows (COM made ready on that thread and let go of after; where the shell cannot
select the file, its folder opens - Tauri's opener does that for the "file not found" it
sometimes answers, this for any failure, the path having been found first), `open -R` on a
Mac, and the file manager's D-Bus interface (`FileManager1.ShowItems`) on Linux, with the
folder opened where no file manager answers; Open is the `open` crate - `ShellExecuteExW`,
`open`, `xdg-open`. Every stored path is `paths::canonicalize`'s, so a share reaches the
shell as `\\server\share`. A video that cannot be played has its way out in the viewer's
menu ("Open in default app"), as in the Svelte viewer, not a button of its own under the
message. A test's system is the stand-in (`common::Picker`'s `opened`, `open_refused`).

**The adapter that draws the window is photon's choice** (`gpu.rs`; spec decision 6): in
wgpu's own order (`order`, held to wgpu-core's `request_adapter` - the GPUs by the power
preference, then an adapter that does not say what it is, a virtual one, a software one
last) over the adapters that can draw to the window's surface, **each tried for a device in
turn** (`native_adapter_selector`): wgpu already takes WARP or llvmpipe where it is the
only one, but does not go on when the adapter it chose cannot make a device, and the window
did not open. The device made to try one is let go of and made again by egui-wgpu. A window
a software adapter draws says so once (`gpu::SOFTWARE`, `launch_notice`; no test, the
tests having no adapter); one that no adapter can draw is said where there is no window
(`platform::startup_failed`). **AccessKit is switched on** (eframe's `accesskit`): every row,
button and face has told it what it is since sub-project 1; a screen reader's press on
what lies under a dialog is an action on a widget by its id, which egui's `Modal` does not
stop, and the application's dropping of what the window answers under a dialog keeps from
acting (`tests/accesskit.rs`, with kittest's `click_accesskit`) - all but a widget that
changes itself as it is drawn (a fold of the People page, the splitter taking the keyboard),
which changes nothing stored. **The surface is eframe's own** (`the_surface_is_eframes_own`):
`WgpuConfiguration::default()` queues two frames where eframe's own options queue one, and
spreading it under the adapter's choice cost every frame a frame of latency.

