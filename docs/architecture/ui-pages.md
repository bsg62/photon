# The window: video, Settings, shortcuts, copies, People page

Part of photon's architecture notes, split out of `CLAUDE.md` (which indexes them). Read the section for an area before changing it: each rule here is a bug that was found once.

**A video is played by the system's own framework, one for each system** (spec
`2026-10-14-photon-native-video-design.md`; `video/mod.rs`, a state module, and behind it
`gst.rs`, `avf.rs` and `mf.rs`, of which one is built): GStreamer on Linux, AVFoundation on
a Mac, Media Foundation on Windows - each a poster at a time (`poster`), a player with its
sound (`Player`) whose frames come out as RGBA, and `can_play`. No fallback decoder: a
spike on all three runners (2026-10-14) had each play H.264 and seek to the frame, and
HEVC everywhere but a Windows without Microsoft's extension, which answers
`Failure::Unsupported` - as does a Linux without GStreamer's plugins, which are looked for
when a pipeline is made: the build links GStreamer's libraries (its core, and the app and
video libraries of plugins-base), not its plugins, and photon starts on a system without
them. A video whose picture has no decoder is `Unsupported` though its sound has one -
`decodebin` errs only when nothing at all decodes, and says the rest in a `missing-plugin`
message (`settled`). **A frame is handed over turned as the video is seen, and each
framework turns a different half**: GStreamer both (`videoflip video-direction=auto`);
AVFoundation the poster (`appliesPreferredTrackTransform`) and not what it plays, which is
turned here by the track's `preferredTransform`; Media Foundation what it plays (the Media
Engine) and not the poster (the source reader), which is turned here by the reader's
`MF_MT_VIDEO_ROTATION` - a backend that turned both would turn twice. The Media Engine's
first frame is black and stamped before nought, and is dropped; its error's code is the
event's *first* parameter. **A paused player still hands over the frame it stands on** -
the first at opening, the one a seek landed on - which GStreamer's sink gives only as its
preroll (and `try_pull_sample` throws the preroll away before it looks, so a paused sink is
asked for the preroll alone), and AVFoundation's video output not at all, as it vends what
the player renders: a paused Mac player makes that frame as a still, as a poster is. **Nothing a player is asked turns the Mac's run loop**: in the window winit
turns it, and turned again in `frame` it held the window five milliseconds a call; the
tests turn it while they wait (`idle`). **`open` waits for nothing**: it is called on the
window's thread (on a Mac it must be the main one), and Media Foundation's engine takes a
second or two to know a video's size on a runner - the player says when it is `ready`, and
what went wrong after it was opened by `failure`. A Windows "N" edition without the Media Feature Pack has no Media Foundation, and starts all
the same: its DLLs are delay-loaded (below, the installers). Every backend lets go of
what it holds when it is dropped (GStreamer to `Null`, the Media Engine `Shutdown`), and
opens the file to read it alone. **`tests/video` is a binary of its own, with no libtest
harness** (`harness = false`, `libtest-mimic` on one thread): AVFoundation's player item
is made on the main thread, and libtest runs each test on a thread of its own. Its
fixtures are four seconds of red, green, blue and white, a second each, so a frame's colour
says which second it is from (GStreamer's times run two frames behind the picture in a file
with an edit list, so a tenth of a second past each edge is let off), and a file turned a
quarter. A test whose decoder the system has not got says so and passes over itself - which
is every test of them on a Linux without the plugins, this machine's included (installing
them is the user's to decide): **only CI runs them**, and a change to a backend is checked
there, on all three. Each runner's nextest step names the codecs it is expected to decode
(`PHOTON_VIDEO_EXPECTS`), and a test that would pass over one of those fails instead:
nextest prints nothing of a test that passed.

**A video's poster is made by the window** (`posters.rs`, a state module): the engine's list
of videos waiting for one, its claims and its crash-loop guard were built for the web view's
thumbnailer and are used as they stand. A thread started at the first frame opens the
session with what `can_play` says - told it can play where it cannot, every video would be
asked of a decoder that is not there, and failed for good wherever the system calls that
broken rather than unsupported - and takes jobs one at a time: the poster at
`video::poster_time` of the library's length, made on a thread of its own so that a decoder
that hangs is left behind at fifteen seconds and one that panics fails that video alone,
fitted within the preview's 1600 pixels and handed over as a JPEG. **One at a time includes
a decoder left behind**: the next job waits for it (`run`'s `behind`) - a source reader on a
share that has stopped answering never returns, and each video timed out beside the last
left one more open for the session. **Known, and left**: photo decodes and poster decodes
now share one process, and each crash-loop guard counts a death of the process against
what it had in flight - so a decoder that aborts photon (rav1d, the OOM killer) twice while
a video's poster is being made fails that video's poster for good, and the reverse; a fix is
photon-core's guard, not this worker's. **What
a failure is reported as is the web view's rule** (`answer`, `video-thumbnailer.svelte.ts`):
the system's lack, a picture of no size and the window closing are `Unsupported`, which
keeps the video waiting for a session that can; the file's failure is `Decode`, fifteen
seconds `Timeout`. **The window answers the job it has before the engine goes**
(`App::close`, `PosterWorker::finish`): a claim left unanswered is counted at the next
launch as a window that died opening the video, and two of those fail it for good. Its
tests drive a stand-in for the system's framework that answers by the file's name
(`tests/posters.rs`), with the time a poster may take as short as a test can wait
(`Posters::patience`); **every other test of the whole application launches with a stand-in
that plays nothing** (`common::no_posters`), since a file called a video is decoded on a
runner with decoders and not on one without, and each system calls the same unreadable
file something else.

**A video plays in the viewer through its player, and its frame is its picture**
(`playback.rs`, a state module; `App::play_video`). The system's video is one trait,
`video::Videos` - `can_play`, `poster` and `open` - over the framework (`video::System`) or
a test's stand-in (`App::with_videos`), and a player is a `video::Stream`. Whether the system
plays at all is the poster worker's answer, put where the viewer reads it
(`App::video_plays`): asked once, off the window's thread. The file is read off it too
(`video_paths`, asked once a video), the player opened on the window's thread - which on a
Mac is the one AVFoundation wants, and is why `open` waits for nothing - and told to play at
once, with its sound, as the web view's `<video>` did. Each frame's newest picture is written
into one texture in place (`TextureHandle::set`) and handed to the viewer as its one level,
so a video's frame is drawn as a photo's picture is, fitted; until the first its poster - the
preview - stands in its place. A frame is asked for every 8 ms while it plays (and once more
after its end, whose last frame can come after the frame that heard it): a crude rate,
which PR 4's pause has to stop - GStreamer could say when a sample arrives, the other two
cannot as simply. **A player is opened only once the viewer has rested on its video**
(`Viewer::rested`, as for a photo's full picture): a held arrow passes videos faster than a
player opens, and each opened and let go of was work on this thread and a moment of its
sound. **A player that fails is let go of** (`Playing::frame`): on Linux a video whose
picture has no decoder plays its sound alone, and kept, it played on under what is said of
it; and the poster, not its last frame, stands under the words. A new picture of the same
video is a new file - followed to a new name, or rewritten - whose path is read again (no
test: the seam is a file moved while the viewer is on it). **Known, and left**: each frame
is made a `ColorImage` on this thread and uploaded from it - about a millisecond at 1080p in
a release build once the needless unmultiplying pass was dropped, four at 4K, and ten times
that in a debug build; and a player has no crash-loop guard, where a poster does - a file
whose later frames crash a decoder takes the window down at each opening, which is not a
loop, since each needs a click. And a player is opened and let go of on the window's thread (GStreamer's
`Null` joins its streaming threads, Media Foundation's `Shutdown` waits), so a video on a share
that has stopped answering can hold the window: the poster worker leaves such a decoder
behind, the player cannot - on a Mac AVFoundation wants that thread. Not reproduced; found by
the switch-over's whole-branch review.

**A video's controls are its strip, its keys and a click on its picture** (`Playing::act`,
`playback::Act` - `createVideoPlayer` ported; drawn by `ViewerView::strip`, `VideoControls.
svelte`). The strip stands over the bar, in the middle of the photo's area - beside the info
panel too - and only while the video plays: play and pause, the time, the position bar, the
length, mute, the volume and the loop, each with no stop of the Tab key. Space, `L`, `M` and
Shift with an arrow (five seconds) are its keys, read before the viewer's, whose plain arrows
stay steps; not in a slideshow, which passes videos over. A click on the picture plays and
pauses (its press stands, where a video's poster's was over at once). **The sound and the
loop are the session's** (`playback::Prefs`, `App::video_prefs`, stored nowhere), and are
set on a player **before it plays**: after, a muted session heard the first moment of every
video. A drag along the bar pauses the video and shows each place's frame, plays on when let
go of, and is called off - back to where it began - by Escape, which is the drag's alone
while it lasts, and by the window being left with the button down. **A drag called off is
over until the button comes up** (`ViewerView::scrub_off`): egui ends a widget's *drag* at
Escape, but not a press that may still be a click - one held still, inside its time - and
the bar went on being held, so the frame after the Escape began the drag again. The first
probe of the flag passed, because its test moved the pointer after the Escape, which is
what ends a click's chance; the test now holds still first. **At its end a video loops, or
stands paused** - as a web view's does: a seek back shows its place and plays nothing, and
the button's first press plays it again from the start; taken for playing still, the strip
said "Pause" over a still video and the window drew on. Paused, nothing is asked for but
the frame a seek is waiting for, and that **for at most `AWAIT_LIMIT` frames, or none at the
end**: a seek to the end brings no frame, nor does one to the frame already shown, which a
player hands over once, and waited for, they kept a still window drawing for good. The strip
takes the photo's area less a margin, as `VideoControls.svelte` does (the buttons at the
sides stand at the middle of the window's height), and where its position bar would have
less than `STRIP_BAR` the volume's slider gives way. A click on the picture is its own action
(`ViewerAction::VideoClicked`): within a double click's time of a menu's item or a dialog's
row closing it is that double click's second press, and pauses nothing (`overlay_closed`, as
for a face's outline). **Another video, or another
picture of the same one, is another player** (`Playing::is_of`): the step from one video to
the next let go of the first. What is said over a video that cannot be played is the web
view's, in its order (`playback::said`): a crash first; then a system that plays nothing -
on Linux naming GStreamer's plugins; then the video's own failure, a missing decoder on
Linux named the same way. Its tests drive a stand-in player (`tests/playback.rs`); which
frame is on screen is read off the texture the viewer last drew (`App::video_frame`), not
off the one kept, which a probe showed is no proof it was drawn.

**Settings is a dialog of the dialog layer** (spec `2026-10-15-photon-native-pages-design.md`;
`settings.rs` for what it holds and says, `settings_view.rs` for what is drawn,
`app/settings_dialog.rs` for what it reads and writes): `Settings.svelte`, word for word. One dialog at a time, so **a question in it is asked
in its row** - Find faces switched off with people named turns its row into
`switchOffWarning` with Cancel and "Stop finding faces", the keyboard on Cancel - and the
library is asked how many are named first (`face_data_summary`, off the window's thread):
with none there is nothing to lose and nothing is asked. **Nothing stored is shown before
it has been read**: the interval, the shuffle and the look-alikes are read as it opens, and
their controls take no press until then, as the Svelte ones are disabled. The interval's
field takes digits and is stored at Enter, at leaving the field and at the dialog closing
- Escape closes it from inside the field - and shows what the library stored, which clamps;
Enter is taken before egui's field sees it, so the caret stays. **The theme is put on the
window at once** (`theme::apply::put`): egui's visuals and the title bar the system draws
(`ViewportCommand::SetTheme`), at launch as at a choice, `System` being no theme of the
window's own so that it follows the desktop. A theme the library will not store is kept for
the session, as the Svelte store keeps it. About reads photon's memory as it is shown and
every two seconds while it is (`MEMORY_POLL_MS`), and at no other time: Settings at any
other section is a still window. Its words for what the figure covers are not the Svelte
ones, which name a web view this window has none of. The section is painted by hand and
scrolled by a number of its own, by the info panel's rules (`settings_view.rs` says which);
a heading's room above it takes in the room already under what stands before it, as one
CSS margin collapses into the next. Not tested: that the title bar is told the theme at
launch (`egui_kittest` draws frames past the first before a test can read its output -
the press is tested, and `put` itself).

**Statistics, the shortcut list and the `?` sheet** (`stats.rs`, `shortcuts.rs`,
`shortcuts_view.rs` - `stats.ts`, `shortcuts.ts`, `ShortcutList.svelte` and
`ShortcutSheet.svelte`). Statistics is counted when its section is first shown and not
before, nor again while Settings is open (a failed count is asked again when the section
is left and come back to); a row closes Settings and is a search, made as a sidebar row's
is (`App::go`, the search box told). **The keys are listed in one place, `shortcuts.rs`**,
and a test reads the native handlers' sources - `Key::…` in the files its
`ANSWERED_IN` names, read by line, not their tests - and fails on a key a handler answers
that the list lacks and on a listed key nothing answers. A handler in a new file is added
there with its group; the wording no test checks. **`?` opens the list as a dialog of the
dialog layer** (`Dialog::Shortcuts`), over the grid, the viewer or compare - not over
another dialog, nor under a menu, nor where a text field or the viewer's keyword has the
keyboard (`?` is a character there), nor under a held band, which has taken every key out
of the frame before it is read. `?` closes it, as Escape does. **Compare is deaf under a
dialog, as the viewer is** - the sheet is the first dialog that can open over it: a pan
held when it opens is let go of (the pointer moving on under the sheet panned the photos),
egui's Tab walk is not called off, and what compare answers in the frames the dialog opens
and closes in is dropped (a click on its cross in the very frame `?` opened the sheet
closed compare under it). The list is drawn in two columns in the sheet, the groups in
their order and split where the columns come out nearest the same height, and in one in
Settings; Settings' section and the sheet's body are one scrolled column
(`settings_view::Column`).

**What a review of the shortcuts found, each with its test** (`tests/shortcuts.rs` and
beside the gestures). *`?` opens nothing over a press held on the photos*
(`GridOutput::pressing`): a press may still become a band, and it did, under the list,
selecting behind it - the rule Enter and `C` already had. *The gate is a text field and an
open dropdown, not whatever holds the keyboard*: `egui_wants_keyboard_input` is any focus,
and the splitter keeps it after every drag, so `?` did nothing after the sidebar was
resized; a dropdown's keys are its own (`Shell::lists_open`, the crop tool's ratio while
the tool is open). *A drag held when a dialog opens is let go of*: the crop tool's handle
(`end_drag` under `paused`) and the zoom's slider (`let_go`, set in the paused frame
before the slider is drawn, and kept after the list is shut with the button still down -
by `?`, since egui ends a drag at Escape itself) went on following the pointer under the
list. *The sheet closes at `?`'s press, and not at a chord*: a held `?` shut it at its
first repeat, and egui's `consume_key` takes Alt+`?` for the key. *The list's tripwire
reads the whole of a file's code*: it stopped at the first `#[cfg(test)]` (a third of
the way into `grid/view.rs`), dropped a key it had no spelling for (`Key::Delete` was no
key to it), and read `GridKey::Home` as the key. Now it stops at the tests' module, a name
it cannot spell is a key no row lists, and `Key::` is egui's only after no letter.

**The view of a photo's copies** (`copies.rs` - `copies.ts`, with its tests' cases):
`GridView::Copies`, a `Step::Copies` on the views' queue, from the info panel's "Show N
duplicates in the grid" and the tile menu's "Show N duplicates", and landed as the Svelte
`showCopies` lands it - the view first, then the photo selected in it and brought into
sight (`App::show_copies`, `Nav::jump`). The menu's item is there from its first frame
where the grid has the photo among the Duplicates (`Deciding::copies`, the entry's
`has_copies`), and the library's count (`copy_count`, asked as the menu opens and forgotten at
the next opening) puts only the number in its words - "Show duplicates" until then, and
without a number for a count of none. **Nothing comes into an open menu with the count**:
it came with it at first, and a menu opened low in the window grows upward from the
pointer (`place_menu`), so the row it gained moved every item up under the pointer, and a
click on "Unstar photo" a frame after the count landed hid the photo (the macOS flake of
`the_star_key_takes_several_photos_the_way_of_the_deciding_one`, caught by a trace of every
star write on a stress loop). A grid whose rebuild changes `has_copies` while a menu is open
still moves its items, and a menu's test waits for the hash pass first (`over_files`). **The engine's argument is the photo and the
hash its content had when the view opened** (`CopiesArg`), which no step can name, so where
the user already is is asked of the photo alone (`nav::copies_of`): asked by the step's
argument, every link to the copies on screen was a rebuild. The sidebar names the view
under Duplicates by the photo's file name, and keeps the name once the photo has left the
library (`copies::keep_name`). **What the view says of its photo is said only beside the
grid it was read with** (`App::copies_notice`, `counted_at`): the counts are read a round
trip after the grid, and with the grid before, a photo just purged was "No other copies
any more" of a photo whose copies were all still there. Over an empty view it is the
view's line; over one that holds photos it is a line under them, in a band at the
content's foot that the photos do not reach (`ShellData::foot`) - not where the Svelte
`.lone` stands, after the photos and scrolled with them, since the native grid lays out
nothing after its rows. **The band keeps its room while its line is a round trip away**
(`foot_shown`): every rebuild of the view - a star, a thumbnail, a scan - is a new grid,
and gone and back at each, the band gave the photos its room and took it again, leaving a
grid scrolled to its end a band short of it (a review found it). Its words are not kept,
being of the grid before; the empty view's line does blink, its words alone. The sidebar
names the row by the photo the counts were read of only when it is the one gone to
(`nav::copies_of`), and the menu offers a count only for the photo it is of now: a menu of
several becomes one of one when a grid takes the others away, and a count asked for an
earlier menu can land in a later one (that race has no seam; the opening's reset of the
count is tested). A tile's copies mark is left off in Duplicates and here
(`copies::marked`), where every photo has copies; the marks are painted as images, which no
frame test reads, so the rule is held by its function's test and the wiring by nothing.

**The People page is not a view of the grid, here as in the Svelte UI** (`people_page.rs`,
a state module - `createPeoplePage` with its tests' cases and `nameChoice`;
`people_view.rs` - `PeoplePage.svelte`, `FaceStrip.svelte`, `NameBox.svelte`;
`app/people.rs`; `face_crops.rs`). The views' queue, `Nav` and the selection know nothing
of it: it is `App::page`, drawn in the grid's place under the sidebar and the top bar,
whose sort and size controls are not drawn over it (their room kept, their lists shut).
The grid is not drawn under it and keeps its place. It opens from the label half of
People's heading (`What::PeoplePage`, the chevron still folding), which is then where the
user is and no view's row is; it is left by any view asked for through `App::go` - every
step but a sort - by a folder, by All photos, and by typing in the search box. **A read of
it is numbered when it is asked** (`PeoplePage::reload`), not when it runs, as the Svelte
model numbers its fetch: a group named is hidden at once and stays hidden until a read
asked after the write's end lands, so a read asked before cannot put it back; library
changes read it at once and then at most once a second (`CHANGE_GAP_MS`), and its own
writes are never held back. A reload keeps the order of the groups on screen. **A face's
photo opens in the viewer over the page** (`open_face`): in the grid on screen when it holds
it and no step is on its way, else in All photos, asked for under the page
(`App::ask_step`, which `go` is with the page's leaving in front) and the photo found in its
grid (`Target::Open`, `take_open`) - or "That photo is no longer in the library." A double
click on a face is two plain clicks on it, not egui's: egui counts the click before as
well, and the click on People a moment before made it a triple, no double to egui. Enter on
a face opens it too. **The crops are cut off the window's thread** by the engine's own
`ThumbCache::face_crop` - the route's WebP, decoded with libwebp (`webp`, at photon-core's
version, the one dependency the page added) - half the cores' cutters and at most four
(`MAX_CUTTERS`: one a core held a preview's decode on each at once), each asked for when its face is first in sight and once an opening; a face whose
preview is not cached is an empty square with a figure. They are let go of when the page
closes. A name box is a field of its own for each group, Enter read and taken before it is
drawn (the lesson of every field here), with what Enter will do under it - "Add to Anna"
coloured, being a merge. **Under a group's box and the single faces' the named people are
offered as a name is typed** (`people_page::name_suggestions`: whose name begins with it,
then whose later word does, at most six; not under a person's new name, where another's
would be a merge nobody asked for by typing a rename - that rule has no test), in an area
over the page, above the box where there is no room below: the arrows move through them,
taken out of the frame before the field is drawn, Enter or a click names the faces as the
one picked, and a letter typed lets go of the one the arrows were on. The arrows are on a
person by who they are (`PeopleView::offered` keeps the id), not by their place in the
list: a read of the page landing between the arrow and Enter can put someone new above
them (a review found it; the race has no seam, and no test). Something the
Svelte page does not have, asked for by the first person to run the native one. A test holds the page's read back by renaming `detected_faces` from a second
connection, the faces' seam (`tests/people.rs`); `native-shot` draws the page
(`native-people-*`), its faces written through the library's own calls after the launch's
scan.

**The corrections are the Svelte page's, and each hides what it acts on until its write
is answered** (`people_page.rs`: `act`, `confirm_all`, `name_singles`, `naming`, and the
same numbering as a group named - written, hidden until a read asked after it lands;
refused, put back with the library's words). A click picks a face or puts it back, in one
strip at a time, and a strip with faces picked has its bar: what its section can do to
them (`Strip::actions`: a suggestion's faces confirmed, taken off or ignored, a group's
or a person's taken off or ignored, single faces ignored, ignored ones brought back), a
name box for single faces (the first one's group named, the
others' merged into the person it made, `write_faces`), "Clear", and how a photo is opened.
"Confirm all" confirms the suggestions on screen, and says "Confirm these N" where there
are more. A person is renamed in their row (`renaming`; the field opens with the name
chosen, so what is typed replaces it, and Enter on their own name changes nothing);
"Merge into…" opens a menu of the others (`MenuOf::Merge`), and a merge and a delete are
asked first, as a question of the dialog layer (`Confirm::Merge`, `Confirm::Person`) that
closes when the person is no longer listed under the name it was asked under - the album's
rule (`App::listed`). **"Show more" fetches a page of the group at a time** (`More`, on a
queue of its own, every page wanted) after what the strip holds, through the section's
filter (a person's suggestions are their unconfirmed faces), and **a read keeps what it
loaded**: each read asks again for as many faces as each strip had shown (`Ask::wants`,
`Loaded::kept`), a strip shown more or fewer of while the read was on its way stays as it
was left (each strip's generation), a face both reads hold is listed once, a strip whose
fetch failed folds back alone, and a selection keeps only the faces still shown. The
faces' writes go through the engine's `write_people`, as every People command does. Not
tested, for want of a seam: that a re-fetch which fails costs its strip and not the page
(`people::read`; nothing makes one call of the library fail and the next succeed).

**What a review of the corrections found, each with its test** (`people_view.rs`,
`people_page.rs`). *A crop is kept for every face the model holds*, the faces "Show more"
loaded among them (`PeoplePage::every_face`): kept by the page's first faces alone, a face
past the twelfth lost its crop the frame after it was cut, and stood an empty square for
good. *The second press of a double click presses nothing on the page when the first moved
it* - a bar's action takes the bar away and what stood under it moves up ("Confirm" became
"Confirm these 5", a group's "Ignore these faces" its own "Ignore") - nor within a double
click's time of a dialog or a menu over it closing (`PeopleView`'s `deaf`, `moved_at`,
`PeopleData::just_covered`; a face picked or put back moves nothing above the pointer and
does not count). *The bar wraps* (`Line`), and the name box in it and the rename field
keep the room of the line under them. *"Show more" asks nothing while its page is on its
way* (`fetching`): asked from what was held, a second press fetched the same page and the
third left one out; and a page that lands after "Show fewer", or for an earlier opening of
the page (`More::opening`, `generation`), is dropped. *"Show fewer" lets go of the faces
picked among those it hides.* *The single faces' box opens empty, with a history of its
own, for each new selection.* A test of the whole application that presses on the page
waits a double click's time first (`press` in `tests/people.rs`).

**What a review of the People page found, each with its test** (`tests/people.rs`,
`people_view.rs`). *A write is no way out of the page*: the album on screen deleted from its
sidebar menu sent `go` to All photos, which left the page - it is `ask_step` now; and Escape
in the search box empties it and leaves the page where it is, as the Svelte box does (only
typing asks for the grid). *What the keyboard moves to is brought into sight*, once, as it
gets there and before the page is laid out (egui moves the keyboard at the end of the
frame, and at a wrap round from the last box to the first in the frame after): a name box
is drawn in sight or out of it and a face half in sight takes the keyboard, so Tab walked on
into groups below the window's edge. *A crop is let go of once the page no longer lists
it* under that key, not only when the page closes - and asked for again when it is listed
again (`Crops::forget`): after "Show fewer" and "Show more" its face stood an empty square
until the page was opened again. **At most `MAX_CROPS` (32 MB) are held**, the least
recently drawn let go of first and never one drawn in that frame; one let go of is cut again
when it is next in sight. *A line of controls wraps* (`Line`): a
group's offer, box and Ignore, and a person's buttons under a long name. *"Show more" stands
under every strip of a group with more faces*. A test that reads where egui
drew a widget after a frame reads the frame before it: egui swaps its two passes at the end
of one (`Context::end_pass`), and `read_response` answers from the older. Not tested, for
want of a seam: a write that panicked puts back every group hidden for a write on its way
(`forget_writes`), and a face's photo asked for under the page is not opened once the page
has been left for another view while its step was on its way (`take_open`).

**Settings' Folders and Tags** (`watched_list.rs`, `tag_manager.rs` - `settings.ts`,
`tags.ts` and `createTagRenamer`, with their tests' cases). **A question is asked in its
row, one at a time** (`settings::Question`: switching face detection off, removing a
folder, removing a keyword, a rename onto a keyword that is there or listed under Changes),
answered by `Asked::Yes` or `No`; asking another is the first one's Cancel - over a
rename's own, that rename declined - and when it is answered the keyboard goes back to
what asked (`SettingsView::back_to`). **Escape is the dialog's close unless Settings has
a use for it** (`settings_view::claims_escape`, asked by the dialog before it takes the
key): a question's Cancel, the keyword's field and the folder's field giving up, a filter
that holds something emptied. The three fields claim Escape in their `EventFilter` - left
to egui it takes the keyboard away at the start of the frame, before anyone can see whose
it was - with all four of its fields said. **The field a folder is typed in** opens where no picker could
be opened ("Add folder…" is the picker, above), with "Choose…" beside it to try again: it
opens with the keyboard, is asked about as it is typed off the window's
thread (`check_folder`, the export field's rules, the text as it stands first), and is
added by Enter or "Add" through `commands::add_folder`; the library's refusal - inside a
folder already watched - stands under the field, which keeps the path. A folder added is
in the list on hand at once and counts as being scanned (`Scans::added`), as a dropped one
does, and a folder dropped on the window while the field is open is put in the field and
watched by nothing. A folder's row says its count of photos only once that is read, where
the Svelte list says "0 photos" until it is; its Rescan takes no press while it is
scanned (`rescan_folder` does nothing then and says nothing back); its Reveal, and About's,
ask the system's file manager.
**The counts and the changes are read again at every change of the library's data** while
Settings is open (`settings_lists`), so a keyword removed from elsewhere, or a scan, shows
in it. A keyword is renamed in its row (`Renamer`): its field opens with its name chosen,
so what is typed replaces it; Enter stores, Escape and leaving the field give it up;
while a rename is on its way the field takes nothing and is not given up; the answer
closes it only if it is still the field that sent it, and a refusal leaves the name with
the library's words. The keywords can be thousands: a row out of sight costs its height
and nothing else. Nothing here writes outside `library.db`: a folder removed keeps its
files, and a keyword's rename or removal is a rule applied as keywords are read.

**What a review of Folders and Tags found, each with its test** (`tests/settings_lists.rs`).
*A folder whose removal is on its way offers nothing* (`App::removing`, `FolderRow::going`)
until the folder list read after the answer: a watched folder's number is handed out again
once it is free, and a second Remove… of the row while the first waited removed the folder
added meanwhile under that number - `App::deleting`'s trap, met again. Refused, it is a row
again (no test: no seam makes the library refuse a removal). *A drop goes into the field
only while the field is on screen and takes a path* (`add_field_shown`): open in another
section, the drop went into a field nobody could see, and nothing was watched; and the
card says so there (`CARD_ADD`). *A question takes Escape only while it is on screen* (the
view's `asking`, of the last frame): left standing in another section or hidden by the
filter it took the Escape that closes Settings, or empties the filter. *Another keyword's
Rename is a rename question's Cancel* (`let_go_of_question`): left standing under the new
field, its Merge agreed to nothing. *The add field has its Cancel*: Escape closes it only
while it has the keyboard. Known and left: between a folder's add and its scan's first
report its row says "Online" and offers a Rescan that does nothing then; a question's
buttons drawn out of sight (to hold the keyboard) are Tab stops there.

**What a review of Settings found, each with its test** (`tests/settings_edges.rs`).
*A press on the scrim closes it whatever else the frame asked*: a tap - press and release
in one frame - had the interval's field give up the keyboard and ask to be stored in that
very frame, and the close was dropped for the ask (`Asked::Close` now goes with it, done
after). *An answered question gives the keyboard back to the box*, as the system's `ask`
does; on nothing, Space ticked nothing. *The interval's field opens with a history of its
own*, the lesson every field here has learnt. *A question asked in a low window is
scrolled into sight* with the keyboard on its Cancel. *A pinned theme is put on the title
bar again whenever the system says the window's theme changed* (`keep_title_bar`): winit
on Windows does not keep the theme it is told and puts the system's back at every
`WM_SETTINGCHANGE` (read in winit 0.30.13's source, not seen run - the smoke checklist's).
*Each opening of Settings has a number* (`Settings::opening`): the count of people named
and the interval's answers act on the opening that asked - over another opening's field
the stored interval does not write over what is being typed - and a count that lands
after the dialog has closed still switches off when nobody is named, as the Svelte switch
went on after its dialog closed. That last has no test: a read cannot be held back, and
the count is one. Known and left: letters typed in the double click's deaf time after
opening are dropped, which cannot happen while the field takes nothing before the stored
interval is read.

