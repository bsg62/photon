# The window: selection, keys, rubber band, writes, menus, fields, dialogs

Part of photon's architecture notes, split out of `CLAUDE.md` (which indexes them). Read the section for an area before changing it: each rule here is a bug that was found once.

**The selection is `selection.rs`, and most of the Svelte store's has no successor**
(spec `2026-10-11-photon-native-grid-interaction-design.md`). The ids picked, the lead
(the photo the keys move on from) and the anchor (the photo a Shift+click ranges from),
each of the two as an offset *and* an id. Every call is given the index on screen: this
window holds the whole `GridIndex`, so a range's ids and the photo a key lands on are
read in the frame that asks, and `fetchIds`, `extendCall`, `reaching`, `picks`,
`adoptLead` and the landing lookups of `setHidden` are not ported - each guards a round
trip that does not exist here. **A new grid of the same view is followed** (`follow`,
given the old index and the new, in `App::adopt`): photos the new one does not hold
leave the selection, the lead and the anchor are found again by id, and a lead whose
photo left lands on the nearest that stays - the first after it in the old grid that the
new one holds, else the last before - and is selected there if nothing else is. It does
not ask what rebuilt the grid, which is why a hide, a star taken off in Starred and a
scan's purge need no rule each. **A grid of other results clears it**
(`Shown::other_results`: another view, search or order). An offset of the lead or the
anchor read after a new grid without `follow` names another photo. The photo landed on
is brought into sight only if the one that left was on screen in the last frame: a scan
purging a photo while the user is elsewhere must not carry the grid back to it.

**The grid has no widget for a tile**: it paints rows, one area senses the click, and
`tile_at` (in `grid/layout.rs`) says which photo a point is on - none in a gutter, on a
header or beside a short last row, and the pinned header's rectangle is taken out by the
view, since a photo lies under it. A click on no photo selects nothing and clears
nothing. The arrows, Home and End move the lead (`move_from`, `nav.ts`'s `move`: with
nothing selected a key starts from the first photo) and the page keys a screenful
(`page_move`). Until this the up and down arrows, Home, End and the page keys scrolled
the grid, and the sideways arrows did nothing; with nothing selected the page keys
still scroll. The grid follows the lead by the least movement, under the pinned
header's height. The keys are read from the frame's events, in order, repeats included -
but not Escape's repeat, and not a chord with Ctrl, Cmd or Alt, but for Ctrl/Cmd+A.

**The keys are the grid's only while nothing else has the keyboard, and had none when
the grid was last drawn** (`GridView::elsewhere`). The grid is drawn after everything
else in a frame and reads the frame's keys, and "unless a text field has them", which
was the rule while its keys only scrolled, had it answer every key a second time that
something else had just answered: the arrow that moved the splitter moved the selection,
Ctrl+A in an open list selected a folder behind it, the arrow that walked the focus from
a button the Tab key had reached moved the lead (a review found all of them, each with a
test). "Had none when last drawn" is for Escape: egui takes the keyboard off a widget at
Escape before anything is drawn, so in the frame of that key nothing has it, and the
Escape that left a control cleared the selection too. What keeps the keyboard by a lock
of its own and gives it up itself - the search box - takes its Escape out of the frame
(`consume_key`) instead. And what lies open over the photos with no keyboard of its own
says so (`GridData::keys`: the search box's help). A click on a photo gives the keys
back, egui taking the keyboard off whatever had it at a click elsewhere. **Every key the
grid gains goes through this one gate**: `.` and `H`, which write, came through it.

egui decides for itself whether a press is a repeat (the key has not come up), whatever
the event says: a test of a repeat holds the key down across frames. The held modifiers
reach egui by `Event::ModifiersChanged`, not by a field of `RawInput`: a test that
clicks with Shift sends it - and a click takes its modifiers from the release's own
event, not from what is held at the end of the frame, in which Shift may have come up.
`egui_kittest`'s `step` draws a frame for *each* queued event. Two events in one frame
of the whole application are put into the frame's input beside the queue
(`harness.input_mut().events.extend(..)`, then a step with nothing queued: `frame_with`
in `tests/dialogs.rs`); a test in whole frames of a view hands them over together.
**The area that senses the click takes
every press on the photos**, however near the scrollbar or the splitter: egui hands a
near miss to a neighbouring widget only where nothing is hit, so the splitter's reach is
its own five points now, as the Svelte one's is. A press held longer than egui's 0.8
seconds is no click (`max_click_duration`, which is also a touch's long press, so it is
left alone). A selected tile's ring is drawn inside the tile. The status bar says "N
selected" before the count from two photos on, and gives AccessKit those words, which is
how a test of the whole application reads them. A test presses a photo where
`App::tile_rect` says it was last drawn.

**The rubber band is a rectangle in the layout's coordinates** (`grid/view.rs`'s `Band`,
`take_band`), so it keeps its grip on the photos it began over while the grid scrolls
under it; its far corner is worked out again in every frame from where the pointer was
last seen in the window. `items_in_rect` says what it touches, by the rule `tile_at`
clicks by (a test walks a grid asking both of every point). The selection holds what the
band started from (`Selection::band_begin`, `band_to`, `band_end`, `band_cancel`): every
frame is that plus what the rectangle covers now, read from the index on screen - which
is why the Svelte band's second answer from the backend, its count of photos not yet
resolved and its abandoning when the grid was rebuilt under it have no successor: a
scan's grid arrives, `follow` takes what left out of the band's two sets, and the
rectangle covers what it covers in the new grid.

**A press on the photos is read from the button's own events, not from what egui calls
clicked and dragged** (`grid/view.rs`'s `Press`, `take_pointer`). The first version
asked egui, and a review found what that costs, each with a test: egui's click has a
time limit (`max_click_duration`, 0.8 seconds, which is also a touch's long press and so
is left alone), so a press held still for a second selected nothing; its drag ends at
the release of *any* button, and at Escape with anything held, so "no longer dragged"
was no evidence that the button had come up - Escape with Ctrl still down, which is how
an adding band is held, kept the band's photos; and after a release that is never heard
of the drag is stale for good, so the next one began no band. Now a press is the
photos' when the primary button goes down with the pointer over them and nothing above
them (`contains_pointer`, and the layer egui finds on top there: below, "one judge"), it
is a click when the button comes up without ever having
been a band, however long it was down, and a band once the pointer is further from where
the button went down than a click may be (`max_click_dist`). The corner a band begins at
is taken in the frame of the press, not in the frame it becomes a band: the wheel turned
under a button that is down and still had moved it two rows.

**A band has one teardown and four endings**: the button comes up, which keeps what the
band covers; Escape, which puts back what was selected before it; the gesture taken
away, which does the same; and the selection cleared under it by a grid of other
results, which leaves nothing to put back (the view finds the selection's band gone and
drops its rectangle). **Escape is read as the key being down that was not**
(`key_down`, an edge kept from frame to frame), not as an event: whoever read the event
first - the search box takes its Escape out of the frame - and whatever is held, the
band sees it; and the band takes the event out too, or the keys after it clear the
selection it has just put back. **A gesture is taken away without a release** in two
ways a window reports: `Event::WindowFocused(false)` - a button let go of over another
window may never be heard of, and a band that outlived it scrolled to the end of the
library selecting all the way, the Svelte band's old bug - and a touch the system
cancels (`Event::Touch` with `TouchPhase::Cancel`, which egui-winit sends with the
pointer gone and no button up; winit reports it on Wayland). The focus being *lost* is
the ending, not the focus being elsewhere: a window with the pointer and not the
keyboard draws bands. A pointer that leaves the window is no ending: it stays a press,
with no position (`latest_pos` is none), and the band keeps the last one it saw - the
edge it was scrolling towards.

**While a band is held it owns the window**: the application takes every key press
out of the frame before anything reads it (`GridOutput::banding`, of the frame before) -
Escape's too, which the band reads from the key being down. The grid's own gate was not
enough - Ctrl+F put the caret in the search box
under a band, and Ctrl+B built the rows again under its corner. A drag on the photos
takes the keyboard off whatever had it, as a click on one does: egui does that at a
click elsewhere and not at a press that becomes a drag. **The corner is a place in the
rows, and rows are built again**: a window narrowed under a held band - a tiling window
manager, the year strip arriving with a scan's grid - is other rows at other heights,
so the press's origin is held by the photo it was on, or the row where it was on none
(`hold_at`, `held_place`), as the grid's own place is by its `Pin`. The scroll at an
edge is the edge's speed times the time since the band's last frame, at most fifty
milliseconds of it, and a frame is asked for only while the pointer rests in a margin
*and the grid can move that way*: at its top or its end the band rests, and whatever
makes room again - the wheel, a longer grid - draws a frame that comes through the same
code. A band begun with Shift adds, as one begun with Ctrl does, though a Shift+click
ranges. No band begins in a grid with no photos, where the shell draws what an empty
library says.

**The native window writes through one queue, and tells the grid only which rebuild is the user's**
(`App::writes`: `Write::Stars`, `Hidden`, `AlbumAdd`, `AlbumRemove`, `PersonRemove`, and
those of the pull requests after it). Each
is one of the engine's own commands, made in its turn off the UI thread, and the only one
that writes outside `library.db` is `commands::set_stars` - the `star=` line in the
folder's `.picasa.ini`. The photos acted on are read from the selection **at the pick**,
in the grid's order, not when a menu opened. What comes after a write is the engine's
rebuild, arriving like a scan's: `Selection::follow` takes out what left and lands the
lead, which is all of the Svelte store's `setHidden` (the landing after a hide, `H` again
going on from there, the last photo's neighbour) with no line here that knows a hide
happened - except that the window does say which rebuild is the user's, so
the grid can glide to it (above, "The window's motion"). What a write's answer adds is only what a rebuild cannot say: a message when
fewer stars were written than asked (`stars_missed`: a folder whose INI cannot be written
costs that and not the photos beside it), the engine's own words for a refusal, what came
of taking photos off a person (`removed_message`), and a read of the lists after an album
changed - `albums_changed` announces a change only while an album is on screen, and the
sidebar's counts are wanted in every view.
`.` goes by one photo, `Selection::deciding`: the lead while it is selected, else the
selected photo that comes first in the grid. Neither `.` nor `H` is answered at its
repeat: each press writes, and a held `H` would hide its way through the library.
**The star's key is the key that types a period, wherever a keyboard has it**: egui names
a key by what it types and, for want of a name for that, by its place, so Shift with the
period's own key - `>` on most keyboards - is `Key::Period` too, and is told from a
period typed with Shift on a French keyboard by the event's `physical_key`.
**A key that writes carries the selection it was pressed on** (`grid::view::Asked`: the
ids, and the deciding photo). The application acts on it after the frame, when the
frame's later keys have been answered: read then, `H` and an arrow in one frame - a long
one, such as puts a rebuilt grid of a large library on screen - hid the photo the arrow
went to (a review's test). And in a frame with a mouse button in it no such key is
answered at all: a click is taken before the frame's keys, whichever came first, and a
write does not guess. **The window waits for its writes when it closes**
(`Queue::finish`, `WRITES_PATIENCE`): dropped with the queue, a hide pressed in the
instant before was lost without a word - and not for ever, since a write can hang.

**A menu is `menu.rs`, `menu_view.rs` and whoever opened it** (`App::menu`, `tile_menu.rs`
for what the tile's holds): `Menu.svelte`, `menu-keys.ts` and `menu-place.ts`. One menu
for the whole window, **shown before anything else in the frame**, because while it is
open the keys are its own, as a desktop's menus have them: it takes every key press and
everything typed out of the frame's input before the search box, the sidebar or the grid
reads it - a cut, a copy and a paste among it, which is what egui makes of their chords
and no key at all (nothing reads one under a held band yet; Copy photo will have to). A
chord too, where the Svelte menu lets one through to the window: Ctrl+F put
the caret in a box behind the menu, where nothing typed arrives. **Tab is not a key that
can be taken out of the frame**: egui decides its walk of the keyboard from the raw input
before anything is drawn, so the menu calls it off (`Memory::move_focus` with
`FocusDirection::None`), and so does the application under a held band. The menu is
placed in every frame from its own size, worked out from its items before it is drawn
(`place_menu`), so items that change under it and a window resized need nothing more. A
press outside closes it at the press, not at the click - a press that becomes a band is
never a click - and the press is left for what it landed on. egui fades a new `Area` in
over a few frames, each asked for: `fade_in(false)`. **The keys are on an item by its
number, and the items are built again in every frame**: when they are other items - a
grid took one of two photos, and two star items became one - the keys go back to the
menu itself (`signature`), or Enter picked what they were never taken to; and an item's
widget is named by its words as well as its place, so a press on one that is another by
the time the button comes up is no click.

**Whether a press is on something is asked of one judge.** egui answers "does this
widget contain the pointer" for one layer: a widget of a layer below still contains it
all along the inside of an area's edge, wherever no widget of the area covers the five
points round the pointer (`hit_test.rs`, `included_layers`). A menu opens with its
corner exactly at the pointer, which rests there - so a click in place was inside the
open menu and the photo's under it as well, and a drag from there drew a band under the
menu (a review found both). The grid now asks both `contains_pointer` and which layer
egui finds on top at the press (`Context::layer_id_at`), and the menu asks the same of
its own layer for "outside", not its own rectangle: egui puts an area on whole points
whatever it is asked for, and two judges half a point apart disagreed about the one
place every right-click leaves the pointer. For the same reason the menu's area is a
point larger all round than the menu drawn in it (`RIM`). An egui widget under an
area needs none of this: an area takes the clicks on its own ground. (`layer_id_at`
locks egui's memory: not inside an `input` closure.)

**The menu is asked for where the other button goes down**, read from that button's own
event as the band's press is (`take_pointer`, `Taken::menu`): egui's click of either
button gives up after 0.8 seconds, and a desktop's own menus open at the press. Not while
the first button is down - a press on its way to a click or a band is not interrupted,
and its release would change the selection under the menu - and not through anything
that lies over the photos, the open menu itself among it (above). On a Mac the first
button with Control held asks for it too (`asks_for_menu`): that is how a mouse with one
button asks, and the system reports it as the first button. A photo
outside the selection is selected first, alone: what a menu acts on is always what is
ringed. Opening takes the keyboard off whatever had it, as a click on a photo does.
**A menu does not outlive the photos it is of.** Its items are built again in every
frame from the selection, so a grid that loses one of several is simply a shorter
subject. It is closed when nothing is selected any more (a grid of other results; a grid
that lost its photos and had none to land on), and when `follow` lands the lead: the
photo the keys were on has left - which is the photo the menu was opened on, unless it
was opened inside a selection of several - and its items could act on the neighbour the
selection landed on, a photo nobody asked about. It closes more often than it must,
never less.

**The sidebar's rows and the grid's headers have menus, and the one menu is whose it
is** (`side_menu.rs` for what an album's, a saved search's and a folder's hold;
`App::menu` is where it was opened and a `MenuOf`). The items are built again in every
frame from the selection and the lists as they are then, so a menu whose album, search
or folder is no longer listed has no items, and closes - the same rule that closes the
tile's menu when nothing is selected. A folder's menu is one list on its row in the
sidebar, on its header in the grid and on the copy of that header pinned over the photos
(`header_at`, in `grid/layout.rs`, says which header a point is on; a period's names no
folder and has no menu). "Delete…" asks before it acts, through the dialog layer
(below); "Reveal in file manager" shows the folder in the system's file manager
(`app/open.rs`, below). The other button on a row is read from its own press (`menu_press`), as on a
photo, and not through what lies over the list.

**A name is typed in the row itself, in the one field the sidebar has**
(`rename.rs`, `createAlbumEditor`; drawn by `sidebar/view.rs`'s `field`). One field keyed
by what it stands in for (`rename::Of`), where the Svelte list has three editors because
its rows' ids collide - album 3, search 3, folder 3. Storing a name is a write on the
queue of writes, whose answer comes frames later: meanwhile the field stays and takes
no typing, and the answer closes it only if it is still the field that was sent
(`Rename::landed`, by the number the field was sent under): a second rename begun
meanwhile is another field. A blank name is a cancel, but a folder's, which is how its
directory's own name is asked back. A name the library refuses leaves the field open,
under a message. **A field whose row is not drawn cannot hold the keyboard**: scrolled
out of sight its name is stored, as leaving it stores it, and with no entry left in the
list it closes; hiding the sidebar stores it too. The list does not follow the grid
while a name is typed, and the field's row is the entry the list is held by. "Rename in
photon…" on a header shows a hidden sidebar and brings the folder's row into sight
(`SidebarView::reveal`). Nothing here writes outside `library.db`: a folder's name in
photon is the library's `alias`, and the directory keeps its own.

**What a review of the field found, each with its test.** *While its name is stored a
field does not ask for the keyboard back.* Stored by Enter it still has it, so what is
typed next does not reach the grid, where `h` hides a photo. Stored by being left it has
given the keyboard to what the user went to: asked back in every frame, Ctrl+F out of a
name left the search box without it, the sort's list closed under the click that opened
it, and when the write landed nothing had the keyboard - the search typed next was the
grid's. *A name out of sight is sent once* (`SidebarView`'s `unseen`): a name the library refuses leaves the field open, still out of sight, and
sent in every frame it was a write and a message in every frame. *A field that was not
drawn in the last frame is opened* when it is drawn - its row scrolled back, the sidebar
shown again (`SidebarView::not_drawn`, which the shell calls for a frame it does not
draw the sidebar in) - and takes the keyboard: a refused name's field came back into sight
without it, and what was typed "into" it was the grid's. *The other button on the row a
name is typed in opens no menu*: it took the keyboard, which stored the name half typed.
*Enter is answered when it is no repeat, and every Enter is taken out of the frame*: the
Enter that picked "Rename…" in the menu is still down when the field opens, and its
repeat stored and closed the field before anything was typed. *A field opens with a
history of its own*: one field under one id serves every row, egui keeps a field's undo
by id, and Ctrl+Z in a folder's field brought back the name typed for an album. *What
was typed is said before what stores it*, by the field (a letter and Enter in one long
frame) and by the shell (a letter and Ctrl+B, which is read before the sidebar is
drawn). *The folder list is read again by a folder's write itself*: the engine rebuilds
nothing when no photo changed, and "Hide folder" on a folder whose photos were hidden
one by one left its menu offering to hide it. *A folder's menu is of a folder the view
lists*, and closes when it no longer is; a menu of the selection closes when the lead
lands on another photo, and a folder's does not. *On a Mac the click of a press that
asked for a menu is not the row's*, whenever Control is let go of: that is kept from the
press (`menu_button`), and can only fail on a Mac - checked by building as one.

**egui's text field, twice now.** What the search box learnt holds for this field, and
one thing the search box had wrong until the second field was built: **a filter built
from `EventFilter::default()` takes the arrow keys away from the field.** The default
leaves them to egui, which walks the keyboard to a neighbouring widget with them
(`TextEdit`'s own filter claims them; replacing it to claim Escape dropped them). In the
search box an arrow key moved the caret *and* handed the keyboard to the button beside
the box, and what was typed next was typed nowhere - since sub-project 2, unseen
because its frame tests draw the box alone, with no neighbour to walk to
(`the_arrow_keys_move_the_caret_in_the_box_and_the_keyboard_stays`, through the whole
application, is the test). Also: Enter and Escape are read and taken out of the frame
*before* the field is drawn (egui's field gives the keyboard up at Enter; an Escape left
in the frame is the grid's next); a press elsewhere stores at the press, by the view's
own rule; an empty field draws its placeholder in the frame it is first typed in.

**A dialog is `dialogs.rs`, `dialog_view.rs` and the application that holds it**
(`App::dialog`): one at a time, over the whole window - the question asked before an
album or a saved search is deleted (`Confirm`), and the keyword picker (`tag_picker.rs`,
`createTagPicker`); the export's and the person dialog have their own paragraphs below. **What a dialog acts on is taken when it opens**: the album or search
of the question, under the name it had; the photos of the picker. A menu's rule is the
opposite, and on purpose - a menu's items are built again in every frame and say what
they would act on now, while a dialog's words were read once, and what the user was told
it would act on is what it acts on (a scan can take a photo out of the selection behind
an open dialog; the keyword is written to the ids its title counted). **By number, which
is not for ever the same thing**: an id of the library's is handed out again once it is
free. For an album and a saved search the window closes that itself (below). For a photo
it cannot - a photo that is hidden has left the grid as well, and is to be written to -
so a picker left open while its photo is purged and another indexed writes to the other
one: improbable, and to be closed only by the library never reusing an id, which is a
migration. The picker closes
*before* its write, which is the user's last word on it, and the message afterwards is
the library's own count and the name it stored, which a rename rule can change.

It is drawn in egui's `Modal`, read from the pinned source: an area in front of
everything with a ground over the whole window, and from the frame after it is first
shown nothing under it is pressed, hovered or given the keyboard - Tab stays inside it -
and `layer_id_at` answers the dialog's layer wherever it is asked, which is the question
the grid and the sidebar ask of a press. **What `Modal` does not keep from what lies
behind is the keys**: the grid, the search box and the shell read them from the frame's
events and not through a widget. So the dialog is shown before anything else in the
frame, as a menu is, and when it has read its keys every key press and everything typed
is taken out of the frame's input. Escape closes it (read first, and taken), and so does
a press on the scrim - the press, as a menu closes. The question opens with the keyboard
on "Cancel", so an Enter that came with the dialog deletes nothing. `Modal`'s area fades
in like any new area, each frame of it asked for: `fade_in(false)`.

**A letter and the Enter after it can arrive in one frame**, when a frame is long: the
field's Enter is read before the field is drawn (egui's field gives the keyboard up at
Enter), which is before the field has taken the letter. The picker's submit carries what
the field holds after the frame's typing (`Done::Submit::typed`), and the sidebar's field
says what was typed before it says the name is to be stored - without it "Porto" and
Enter in one frame renamed an album to what it was already called.

**Three more things about a dialog's keys, each found by asking of the dialogs what a
review had asked of the sidebar's field.** *A key held when a dialog opens presses
nothing in it*: egui presses the button the keyboard is on at every Enter and Space,
repeats among them, so the Enter that picked "Delete…" in a menu, held a moment longer,
pressed "Cancel" on the question it had just asked; the dialog takes repeats of the two
out of the frame before it draws. *A key in the frame of the Escape that closes a dialog
is taken out with it*: the dialog returned at Escape before it had cleared the frame,
and an `h` behind the Escape was the grid's. *The picker's field opens with a history of
its own* (a fresh `TextEditState`): it has one id for every picker, egui keeps a field's
undo by id, and Ctrl+Z in a new picker brought back the keyword typed in the last.

**What a review of the dialogs found, each with its test.** *A delete on its way takes
its row's menu away* (`App::deleting`, `going`): an album's number is handed out again
once it is free, the row stays until the lists are read after the delete's answer, and
"Delete…" asked of it a second time waited behind an album made meanwhile under that
number - and deleted it. Refused, the row is a row again. *A question whose album or
search is no longer listed under the name it was asked under closes* (`App::listed`):
deleted from elsewhere under the question and another made at once, "Delete the album
“Porto”?" answered with yes deleted Paris. *The second press of a double click does
nothing in a dialog* (`DialogView::deaf`): a double click on a menu's item opened the
picker at its first click and landed its second on a keyword row, which writes at a
click - a button that goes down within egui's double-click time of the dialog's first
frame is that press, and nothing is done until it has come up. A test that presses in a
dialog lets that time pass first (`pick_item`, the fixture's `open`). *Enter writes the
keyword the library has under another case* (`TagPicker::submit`): the list's first row
is "Lisbon" for "lisbon" typed, and Enter wrote "lisbon" beside it - where the Svelte
picker still does. *Nothing under a dialog acts*: egui keeps the pointer from what lies
under a modal through the modal's own ground, which it finds under the pointer only from
the frame after the one the dialog is first drawn in (it tests a press against the
widgets of the frame before; the "modal layer" in its memory is about the keyboard). So
in a frame drawn under a dialog the dialog takes a button going down out of the frame's
events, which is where the grid and the sidebar's menus read it, and the application
drops whatever the shell answers - a click that was over within that first frame was
still a click on the sidebar's row. *The cross in the picker's corner shows the keyboard
on it*, as every button does. Not closed, and known: a screen reader's press goes
through egui's modal, and the application's dropping of what the window answers under a
dialog is what keeps it from acting (`tests/accesskit.rs`) - a widget that changes itself as
it is drawn, a fold of the People page, still takes it; the picker lays out every keyword in every frame, which is slow in a library of thousands;
and a keyword's removal rule is dropped by adding that keyword again, as the engine has
it.

