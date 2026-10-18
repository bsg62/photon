# The window: viewer, info panel, colour, faces, crop, slideshow, compare

Part of photon's architecture notes, split out of `CLAUDE.md` (which indexes them). Read the section for an area before changing it: each rule here is a bug that was found once.

**The viewer takes the whole window** (spec `2026-10-12-photon-native-viewer-design.md`;
`viewer.rs` for which photo is open, `viewer_view.rs` for what is drawn, `App::viewer`
while it is open). The shell is not drawn under it, so the grid's keys cannot be answered
twice and the grid keeps its place, which is a number of its own. Enter on the photo the
keys are on and a double click open it - **a double click is read from two plain clicks
on one photo** (`GridView::clicked`), not from what egui calls one, which is of a widget,
and the photos are one widget. Escape, Backspace, the mouse's back button and the cross
close it, and the photo it was on is handed back selected and brought into sight; one the
view has lost hands back none, and the grid is brought to where it stood. **Enter opens
nothing in a frame with a mouse button in it, nor while a press on the photos is held**:
opened over a press, the button came up where the grid did not hear it and the first move
of the mouse after the viewer drew a band with no button down; opened in the frame of the
other button, it stood under the menu that button had asked for. **Other results that do
not hold the viewer's photo close it** (`adopt`: a search typed, and a photo opened before
it was sent): its offset is a number of another list, and the step back from the ninth of
all photos went to the third of three. Where they hold it too, it stays, at its place in
them. And a search typed is sent on time under the viewer, which asks for its frame as
the shell does. **Most of the Svelte viewer's guards have no successor**:
this window holds the whole `GridIndex`, so the photo is found again by its id in the
frame a new grid arrives in (`Viewer::follow`), and a picture is asked for by the photo
and its key (`Want`), never by an offset - `createLoadSlot`, `rebind`, `renumbered` and
`detailsSeq` are not ported. A photo the new grid lacks has left the view or the library,
which only the engine can say: `viewer_item` is asked again, an answer makes it
`orphaned` (it stays on screen, with no place in its caption, and the next step goes on
from where it stood) and the refusal of a photo that is not there (`notFound`) `gone`;
any other refusal leaves what is on screen, which nothing tests - there is no way to
make the library fail a read on demand. `picture_changed` is ported whole, with the
faces left out of what it compares. **What the library says of the photo is a round trip
behind the grid**, and is what `.` and `H` go by: a test that presses one waits for the
facts first, as a user's eye does.

**No control of the viewer is a stop of the Tab key** (`Sense::CLICK`, not
`Sense::click()`): a control that had the keyboard would take the arrows from the viewer,
egui walking the focus on with them, and every control has a key. **And Tab is called
off in the viewer** (`move_focus`, as under a menu; not under a dialog, whose own walk it
is then - below, with the faces): a message drawn over it has a button
that is a stop, and with the keyboard on that the arrows stepped nothing and the grid's
keys were dead once the viewer had closed. The wheel steps a
photo a notch whatever the system's lines a notch are - nothing scrolls here - and a
trackpad's stream adds up to one (`viewer::wheel_step`); what it has turned is the
viewer's it was turned in (a view made anew at every opening).

**The picture is the preview, then the full picture, and photon scales the full picture
itself.** The preview is the cache's 1600-pixel thumbnail (`Thumbs::previews`, a second
store over `EngineThumbs::preview`). The full picture is `commands::viewer_picture`, a
command the Tauri photon never called: `render_picture` behind `RENDERING`, as pixels.
It is asked for on a `Latest` - one at a time, the last one wins, since a decode cannot
be taken back - and **after a rest of 130 ms on a photo stepped to** (`Viewer::rested`):
a held arrow key passes photos faster than they decode. **egui's wgpu renderer gives a
texture one level** (`mip_level_count: 1`, whatever `TextureOptions::mipmap_mode` says),
so a camera's photo fitted into a window is sampled from a few of its pixels in every
ten. Two things answer that, and the second was added when a picture showed the first
was not enough: the picture's halves (`viewer_levels.rs`, each the mean of four pixels,
down to 512 pixels - at 2,048 an upright photo in a low window was drawn from a level
three times its size; the level drawn is the smallest at least as large as what is
drawn), which are what a zoom draws from; and **a copy scaled to the window with a
Lanczos filter** (`edit::fitted_copy`, made by the engine for the box the viewer names,
drawn pixel for pixel on whole pixels while the photo is fitted at that size). On a zone
plate - `native-viewer-quality.png` beside its reference, which `native-shot` writes -
the halves alone drew rings the photo does not have; with the copy the two pictures are
the same. A window of another size is another copy, asked for once the size has stood
for 200 ms, and the levels are drawn meanwhile. A level larger than the device's limit
is not made into a texture (`max_texture_side`; **`egui_kittest` says nothing of it and
egui then takes 2,048**, so a test that wants a large level says
`harness.input_mut().max_texture_side`). **Nor is a level larger than twice the photo's
area, until a zoom outgrows the largest held** (`PictureAsk::max_side`,
`FullPicture::short_of`): the whole picture is then made again, the largest level held
drawn until it lands, and kept for that photo - a resize while zoomed makes the copy as
deep as what is held. A camera's every level was a fifth of a GB of textures for a zoom
most photos never get; the price is a soft picture for a decode on the first deep zoom. Not
kept in memory beside the textures instead: egui keeps no copy once a texture is uploaded,
so that would only move the levels from the GPU's memory to photon's, which on an
integrated GPU are the same. The picture is handed over as `ColorImage`s
made on the worker: the frame that uploads a 10 MP photo and its half took 8 ms in an
unoptimised build, so it is one frame's work and is not spread over several (that is
egui's pass and the write of the textures, off screen; what the GPU does with them
after is not in the number, and has not been measured in a window). **A full
picture that cannot be made leaves the preview**, with nothing said: a photo on a drive
that is not plugged in is looked at from photon's own cache, as the web view left it;
why is said only where there is no preview either. A picture made for a photo the viewer
has left is dropped as it arrives. A test holds a render for as long as it needs by
holding `RENDERING`.

**Whether the full picture is asked for is decided from what is held and what is on its
way, never from what was once asked** (`show_viewer`: `none`, `other_size`). "Asked for
once" left a photo at its preview for good in three ways a review found: one photo on
and straight back inside the rest (the picture is let go of at the step), a photo that
was gone and came back, and a picture the engine made from another state of the photo
than the one named (`Made::Stale`) when the photo is back under the key it had - turned
and turned back between two frames, the window never hears it was another picture, and
no grid asks again. So a stale answer has the library read again, and the picture is
asked for after that answer, under the key the engine has by then. What stops the
asking, each with its test: a photo that is gone (asked for then, the refusal would be
what is remembered when it is back), one the library says cannot be shown (its picture
is asked for at the opening, before the library has said anything, and is not held when
it arrives), and a picture that could not be made - until the photo is come back to. The
question on its way is kept beside the task and not with the viewer (`App::asking`),
which can close and open again on the same photo meanwhile. **A viewer closed holds no
texture**: its previews are let go of with it (`Thumbs::clear`), which nothing else did
but their store's own limit of ninety-six megabytes.

**The zoom and the pan are the viewer's state, and the arithmetic is `nav.ts`'s**
(`viewer_zoom.rs`: `zoom_at`, `clamp_pan`, `actual_size_zoom`, `wheel_zoom_factor`, with
that file's tests' cases; `Viewer::zoom`). A point `q` of the fitted photo, measured from
the middle of the photo's area, is drawn at `pan + zoom * q`: everything is measured from
that middle, not the window's, which is another place once the info panel stands beside
the photo. The fit again at a step and at another picture of the same photo
(`Viewer::land`, `took`), in the state module, so that nothing in the application has to
remember to; kept at a star, a new grid, a photo leaving the view. The view is handed
the zoom to change (`ViewerView::show(.., &mut Zoom)`, a copy the application writes
back before it acts on the frame's steps). **What zooms**: `+` `=` `-` `0` about the
middle; the wheel with Ctrl or Cmd, a fifth a notch, at the pointer - read from the
wheel's own event, a line taken for a hundred of a web view's pixels, because egui's
`zoom_delta` is the same turn scaled by its scroll speed, which is the grid's and
another on every system; a pinch where the system reports one (`Event::Zoom`, a Mac's
trackpad); the slider, which leaves the pan where it may stay; a double click, to the
photo's own pixels and back. **"Its own pixels" are what the library says the photo
has**, turned as it is seen, whether or not the full picture has landed - a double click
a moment after a step goes where one a second later does (the first version went by
the largest level on hand, and to twice until there was one); until the library has
answered, the largest level. At that zoom the level stands on whole pixels (`picture`'s
`own`): half a pixel off them is the linear filter's blur over the whole photo, at the
one zoom a photo is checked for sharpness at. The fitted copy is drawn at fit and at no
zoom, which is said in so many words (`zoom.is_fit()`): a zoom can be the copy's size
too, a hair over the fit or twice in a window half as wide as the copy was made for.
**The fit is asked as a question, and a float's error over it is the fit**
(`clamp_zoom`, `FIT_WITHIN`): a fifth out and a fifth back a few times over ends at
1.0000001 in single precision, where `nav.ts`'s doubles end at one, and everything that
asks "is it zoomed" then said yes - a hand over a photo that cannot move, and a double
click "back to the whole photo" with nothing seen to happen. **A press on the photo is
read from the button's own events**, as the grid's band is (`Press`): a click when it
comes up within a click's distance of where it went down and never went further; a pan
while it is held, at once and not after a threshold, **if the photo was zoomed when the
button went down** - a press on a fitted photo is no drag and does not become one when
the photo is zoomed under it, or the photo is thrown by all the pointer moved while
there was nothing to move; and a double click from two such clicks in egui's time and
distance. Its endings are the band's: the button up, the window left
(`WindowFocused(false)`), a touch cancelled; the pan stays where the drag had brought
it, and the slider's drag ends with it (`let_go`: it goes through egui's press, which
knows no such ending). Where nothing zooms any more - a video stepped to under a held
button - the press and the first click of a double click are over. A zoom made during
a drag moves the drag's origin along (`Zoom::to` answers how far the pan moved), or the
pointer's next move puts the pan back. **The photo has a surface widget under the
controls** (`viewer-photo`): with something there to be hit egui hands no near miss to
a button, a press is the photo's when it is on no control's rectangle - the photo's own
caption among them - and under nothing that lies over the viewer (`layer_id_at`: a
message's button), and one place decides. No zoom for a video, nor where something is
said in the picture's place; the slider is not drawn for a video, and answers the first
button alone. The hand is closed while the photo is held, wherever the pointer goes
with it. **A zoom's key pressed after an arrow in one frame is lost**: the view changes
the zoom of the photo the frame began on, and the step, acted on after the frame, puts
the next one back to the fit - a frame long enough to hold both costs a second press.
**Ctrl with `+`, `-` and `0` scales nothing** (`zoom_with_keyboard` off): egui would
zoom the whole window by them, as a web view zooms its page, which the Tauri photon has
switched off.

**The info panel is the viewer's while it is open, and the photo is fitted beside it**
(`viewer_info.rs` and `viewer_histogram.rs` for what it says - `exif.ts` and `histogram.ts`,
with their tests' cases; `viewer_layout.rs` for where it stands - `info_room`, `photo_area`
and `info_place`, from one set of numbers; `viewer_info_view.rs` draws it and answers what
was pressed in it). Closed at every opening, kept across a step. The photo's area is the
window less the panel's room, and everything that was measured in the window is measured in
it: the fit, the copy the engine is asked to scale (`show_viewer`'s `within`), the zoom's
bounds and the point it holds still, the pan, a message in the picture's place, the button
for the next photo. The picture is cut off at the area's edge, so a zoomed photo stops at
the panel. The photo's own caption is not drawn beside the open panel, which has it whole.
**Nothing is shown before the library has said it**: a keyword taken off is gone when the
library says so, a round trip later, where the Svelte panel shows its own guess at once and
puts it back on a refusal. Here a refusal is a message - and the keyword that was typed is
not put back into the field, as the Svelte panel puts it. A keyword is `Write::TagAdd` or
`TagRemove` on the queue of writes, for which the engine rebuilds the grid, and a grid's
arrival is what has the viewer read its photo again; one the photo has already, in whatever
case, is not asked for (the engine would rebuild the grid for nothing). An album's tick is
the menu's `AlbumAdd` or `AlbumRemove` with the one photo, and **the write's own answer has
the photo read again** with the lists: the engine announces nothing for an album that is not
on screen. **What the panel writes is written before the frame's steps are acted on, to the
photo the panel was drawn for** (`show_viewer`'s first pass over what the frame did):
written in their turn and by the viewer's photo, a tick in the frame of an arrow key - one
that is held repeats thirty times a second - went to the photo stepped to, whose panel
nobody had seen, and a tick in the frame of the Escape that closes was not written at all.
It is the grid's rule for a key that writes (`Asked`), met again. What the People section
says of a photo with no face is the engine's switch (`Engine::face_detection`, a flag in
memory) and whether a pass is running (`Scans::looking_for_faces`).

**A link of the panel that leaves the viewer shuts it first**, and goes where a row of the
sidebar would, through `App::go`, which tells the search box: a camera, a lens and "Photos
nearby" are searches, a person is their view. The photo is not handed back to the grid where
the grid is being left too; **where the link leads to where the user already is** - a
camera's link in that camera's own search, a person's in their view: `go` asks for no step
there - the viewer closes as it does by its cross, onto the photo it was on. **A copy is
found in All photos** (`App::locate`): by the folder jump's own means, `Nav::jump` with a
`Target::Photo` - looked up in the grid on screen when no step is on its way, else in the
grid of the step to All photos - and there selected, alone, and brought into sight
(`take_photo`'s `locating`, by its id in the grid the next frame draws). All photos is asked
for whatever is on screen: `go` asks for no step to where the user already is, and drops a
search half typed, which would land after the jump. From Hidden too, where a folder asked
for is looked up in the grid on screen: a copy the panel lists is never a hidden photo, and
a photo unhidden in the viewer stays on screen in Hidden with its copies listed - looked up
there, the press closed the viewer and found nothing. "Show N duplicates in the grid" is
the view of the photo's copies (below); "Open in OpenStreetMap" opens the place in the
browser and leaves the viewer open (`InfoAction::Map`); no text of the panel can be
selected. Known and left: a double click on a copy's link lands its second click on the grid
that is by then under the pointer, and selects that photo in the copy's place.

**The panel is one column painted by hand** (`Page`: where the next thing goes; `Flow` for
what stands side by side and goes on to the next line), scrolled by a number of its own,
through a child cut off at the panel's edge - an icon paints through the `Ui` it is given
and a widget is pressed only inside its clip. The cut is what keeps a thing that is scrolled
out from being drawn or pressed; **`Page::sees` is what keeps it from AccessKit**, by not
handing it to egui at all - without it a line that was not on screen was there for a test of
the whole application to find, and will be for a screen reader. (A probe that took one such
test away passed, because the row it looked at was guarded twice: the test now asks for a
name, a row, a link, a heading, a keyword with its cross and an album's tick. Not for a
Picasa album's row, nor for the words between two links.) **But the keyword's field is drawn
in sight or out of it**: egui lets go of a field that is not drawn, and scrolled out of
sight it gave the keyboard to the viewer, where the next letter typed is a key. It then has
the keyboard with no caret to be seen, and nothing scrolls it back when it is typed into.
**The panel's ground is a widget that does nothing, and is needed**: where a press hits
nothing egui hands it to a widget within five points, so a press in the panel's own margin,
just above a link the edge cuts off, pressed the link. In the viewer the photo's surface
lies under the panel and would do as much, which is why a probe that took the ground away
passed until a test pressed there in the panel alone. The wheel over the panel is the
panel's and steps to no photo: **a notch moves it as far as it moves the grid**
(`scroll_bar::turned`: egui's `line_scroll_speed`, which the application sets to the
system's notch in every frame), and a trackpad's points as they come. It was forty points
whatever the system said, which the first person to run the People page on Windows found
"slow" beside the grid, the page going by the same rule; at a Windows set to scroll no
lines a notch the panel's scrollbar is the way down it, and where it pages ("one screen at
a time") a column pages by its own height (`scroll_bar::notch_points`, from the `Notch` the
application keeps in egui's memory every frame): a page of the grid skipped half a
Settings section a notch. **Every column painted by hand
has the grid's scrollbar** (`scroll_bar.rs`: the info panel, the People page, Settings'
section, the `?` sheet) - as wide, held where its thumb is taken, a press on its track
taking the column there - asked before the column is filled, so that what it draws is
where the bar put it; it had been a mark three points wide that nothing could take hold
of. A press on the panel is not the photo's (`Controls::panel`). A panel
that has got shorter than it is scrolled - a keyword taken off - is held to its new end in
the next frame, which it asks for: what it holds is measured as it is drawn. Another photo
begins at the top, with nothing typed. **Its words break at a word's end**, and inside a
word only where the word is longer than its line, which egui does by itself: told to break
anywhere it never looks for a word's end, and a camera was "Canon EOS 5D Mark IV Special E"
over "dition". They are laid out by egui and do not go through `text::paint_line`: a name in
another script is drawn in the order it is written - turned round, in the same window in
which the bar has it right.

**The keyword's field is egui's text field a third time**, with the filter spelled out
(above, "egui's text field, twice now") and a history of its own for each photo - one field
under one name serves them all, and Ctrl+Z in the next photo's field brought back the
keyword typed for the last. Enter is read and taken out of the frame before the field is
drawn - egui's field gives the keyboard up at an Enter it sees, and the next keyword is
typed straight on - and adds what the field holds once it has taken the frame's letters.
**Whether the keyboard is the field's is the panel's own word** (`InfoView::held`,
`typing`), not egui's: egui lets go of a field in any frame that does not draw it, and at a
step there are such frames, as many as the library takes to say what the next photo is. By
egui's word the field kept the keyboard across a step of the wheel when the answer was quick
and lost it when it was not, and "hills" typed next was a keyword or five of the viewer's
keys - `h` hides the photo - by a matter of milliseconds. It keeps it now, for the next
photo's keyword, and asks egui for it back when it is drawn again; letters typed in the
frames between are dropped. **While it has the keyboard the viewer's keys are the field's**:
the viewer reads its keys from the frame's events and not through a widget. Escape leaves
the field and closes nothing, by that rule and not by being taken out of the frame, which
was written first and did nothing: the viewer has read the frame's keys before the panel is
drawn. The next Escape closes the viewer. A press of any button anywhere but on the field
leaves it at the press - egui gives the keyboard up at a click elsewhere, which a press that
becomes a pan is not - and one on the field itself does not, button down or up. Known and
left: a letter in the very frame of the click that gives the field the keyboard is the
viewer's too (the question is asked before the panel is drawn), and a keyword the library
has on another photo under another case is written as it is typed, where the picker writes
the library's. **The field goes with the viewer, in the first frame that finds the viewer
shut** (`App::ui`, before the shell is drawn): egui lets go of a field that is no longer
drawn only at the end of the frame that did not draw it, so that frame's keys were not the
grid's. In one place, because the viewer is shut in two - by its own frame, and by `adopt`,
when a grid of other results does not hold its photo. Each has a test with the key in that
very frame - for the first with the mouse's back button still held, since the click of a
button let go of has egui take the keyboard off the field by itself; with the key a frame
later, as the first version had it, the test passed with the rule gone.

**egui paints a field's placeholder in the theme's "weak" text colour, whatever colour its
words are given** (`hint_text` with a `RichText` of a colour: the colour is replaced). Two
things follow. `theme::install` sets that colour to the palette's dim text, which every
field here had asked for in vain and got egui's greyed-out text instead. And **the viewer's
keyword field is drawn in the dark theme's visuals, whatever the window's** (`set_visuals`
on its own `Ui`): the viewer's glass is dark in both themes, and in a light window "Add a
keyword" was all but invisible - seen in a picture, enlarged, after every test had passed -
and the caret was that theme's dark blue on the dark glass. A placeholder's colour and a
caret's are things a frame test can ask (the fixture's `tints` and `lines`).

**The bar keeps its width while the caption is not known** (`bar_place`'s `was`): at a step
the caption is empty for the round trip the library's answer takes, the bar shrank to its
tools, and its buttons jumped twice at every step - found by a test that pressed the panel's
button a frame after a step and missed it. A test that presses in the bar after a step waits
for the photo's facts. **A fixture's photos have no camera and no copy**, and no call of the
library's gives them one: a test writes the columns itself before the launch (`doctored`,
`copies` in `tests/viewer.rs` - the engine's own pass does not read a drive that is not
there, so the hash stays). And a library where photon looks for faces is still recognising
them for a moment after the launch: a test of the People section's "No faces found." waits
for the pass's line to go.

**The viewer's full picture is converted to sRGB, and nothing else is**
(`photon_core::colour`; `commands::viewer_picture` calls `in_srgb`, and nothing else does).
A decoder hands over numbers, and what colours they are is said beside them: a phone's photo
is Display P3 and a camera's often Adobe RGB, and their numbers read as sRGB are duller
colours than were taken. A web view converts them; the native viewer draws the pixels
itself. The conversion is moxcms: pure Rust, and in the build already as `image`'s own
dependency. `embedded_profile` reads what the file says - the ICC profile of a JPEG, a PNG,
a TIFF or a WebP, through each format's own decoder, and an AVIF's `colr` box, which is a
profile or four numbers (`nclx`). It is a read of the file of its own, after the decode:
every full decode goes through `decode_image`, and its other callers want no profile. **A
PNG's profile is read under a limit** (`PROFILE_LIMIT`): it is deflated, `PngDecoder::new`
inflates it with no bound, and a file of four megabytes took 1.3 GB to say it had no
profile. **Only a profile that says what a picture's numbers are is applied**: RGB, and of a
device or a colour space - not a link between devices, an abstract profile or named colours;
not a CMYK one, whose numbers the decoder has made RGB of by a rule of its own; and not a
grey one, which a web view does apply (the spec's "RGB profiles only": a grey photo with a
profile of its own keeps its numbers, 128 where a browser shows 146). **A profile's curves
are its curve tags, whatever its `cicp` tag says** (`allow_use_cicp_transfer` off, where
moxcms's default is on): by the tag, a profile with sRGB's curves and a tag saying PQ turned
mid grey to near black, and one saying BT.709 was read by BT.709's own formula. **An AVIF's
numbers** are converted where they name a standard-range space: the primaries by moxcms's
table, BT.709's where none are named (each of the two has a default of its own), and the
curve taken for sRGB's own where it is BT.709's family or not said at all, as a web view
takes it - by its own formula a photo that says "BT.709" comes out lighter than every
browser shows it, 128 as 140. PQ and HLG are left as they are: high dynamic range is not
shown on this surface by a conversion of numbers. **A profile that is sRGB is told by what
it does**, not by its name: every step of grey and of each channel alone is converted first,
and where none moves by more than a step the picture is left alone - which is nearly every
profile there is. (Six steps of each were tried at first: a curve that is a table, sRGB's at
those six and lifted between, passed for sRGB with its greys seventeen steps out.) The
picture is converted in place, a chunk at a time, so no second picture of 96 MB is held (30
ms for 24 million pixels in a release build, 27 in one pass over two pictures; about three
seconds unoptimised, which only a debug build of the application pays - moxcms is not among
the crates the dev profile optimises), and **before the fitted copy is scaled**, so that the
two pictures of one photo are of one colour.

What is *not* converted, each on purpose or for want of a decision: the preview (so a
wide-gamut photo changes colour a little when its full picture lands, as it does in the
Tauri viewer), the grid's thumbnails, an export and the copy for the clipboard - whether
those should be is a decision about photon-core, not this viewer's. (It
is not a small one: a re-encoded export of a Display P3 photo carries Display P3's numbers
and no profile, so every other program shows it duller. That is older than the native
viewer.) And "converted" means converted to sRGB's numbers: whether the system takes the
window's surface for sRGB on a wide-gamut display is not something a test here can see (the
smoke checklist, for a Mac). **Known, and left**: an AVIF with no `colr` box whose AV1
stream names its colours is not converted (a browser reads the stream's own header; photon's
decoder reads it for the matrix and the range only, and writers put the box in); an AVIF's
profile is passed over when a box of numbers stands before it (the parser hands over the
first of the two; libavif writes the profile first); a PNG's `gAMA`, `cHRM` and `cICP`
chunks are not read, so one that says its colours that way and carries no profile is taken
for sRGB; and a picture of sixteen bits is made eight before it is converted, which leaves a
wide-gamut ramp a few shadow levels short.

Things about moxcms, `image` and the tests, each learnt by a test that failed or a review
that measured. moxcms's `update_rgb_colorimetry_from_cicp` answers false whatever it did
(0.8.1), so the profile is made by `new_from_cicp` and asked whether it has primaries and
curves (`is_matrix_shaper`). **A profile moxcms writes carries a `cicp` tag**, so a test
built on one alone never takes the road a phone's profile takes; the tests of the tag write
their profiles a byte at a time (`colour::by_hand`, a reviewer's writer). A profile moxcms
writes for any class but a screen's or a camera's cannot be read back (it counts a tag it
then does not write), so the tests of "a device link is not applied" patch the four letters
of a good profile's header - made by the writer, such a fixture was refused for being broken
and proved nothing. `image`'s generic reader loses the profile of a *small* TIFF (it has the
tiff crate take no tag with more values than the picture has bytes), which this module's
16-pixel fixtures are and no photo is. And the values the tests expect are worked out by
hand - primaries to XYZ and back through sRGB's, each space's own curve - not asked of
moxcms; they agree with it, and with lcms2, to the digit. A profile that makes moxcms panic
costs that photo its full picture and no more: the task catches it and the preview stays -
none was found in half a million mutated profiles.

**The faces of the photo in the viewer are outlined in the picture as it is drawn**
(`viewer_faces.rs`, which is `faces.ts` with its tests' cases; drawn by `viewer_view.rs`'s
`faces`). While the info panel is open each face has its outline at the rectangle `picture`
answers, zoom and pan in it, cut off at the photo's area, a named one with its name on a
plate under its left corner; with no picture drawn there is no outline, and no face under a
press. The line, its ink and the plate's ground are the photo's own colours in both themes
(`PHOTO_LINE`, `SHADOW_INK`, `SCRIM`). **An unnamed face photon found is a button**
(`viewer-face`, "Name this face"), as much of it as is in the photo's area, and to the
photo's own press a control (`Controls::hold`): no pan begins on it, and it is no half of a
double click. A named face's outline, and one that is Picasa's alone, take no press - a
press there is the photo's. One that a zoom has taken out of the area is not handed to egui
at all, nor is its plate, so it is not there for whoever reads the window without seeing it.

**Which outline a click is on is the view's to say, not egui's** (`faces`' `which`). egui
gives a click to the last widget made under the pointer that takes one, whatever lies over
it that takes none: the bar's glass is no widget, its caption senses no click, and a button
that leads nowhere - "Next photo" on the last photo, where a hand that has clicked through
a folder clicks once more - senses none either. Each of those named the face under it. And
of two outlines one inside the other egui's click is the later one's, not the smaller's,
where the menu is the smaller one's (`face_at`). So a click names a face only on no control
(`Controls::over`), and the smallest outline that holds it. An outline's widget is named by
its number among the faces and not by its id: two outlines Picasa drew over one detection
carry one id, and under one name both were one widget at the first one's place. **And a
press is asked of the faces as the last frame drew them** (`ViewerView::faces`, `Aimed`),
not of where this frame is about to: in the frame after the panel is toggled, the window
resized or another picture landed the faces are elsewhere, and a press on one face was
taken for another's.

**The other button asks for the viewer's menu where it goes down** (`ViewerAction::Menu`;
`viewer_menu.rs` for its items, in `Viewer.svelte`'s words and order), read from the
button's own event as on a photo of the grid: on the photo's surface and its faces, not on
a control, not under anything that lies over the viewer, and not while the first button is
down - on the photo, or on the slider, which would go on following the pointer under the
menu. (Not once the window has been left with that button down, `let_go`: its release may
never be heard of, egui goes on calling it down, and no menu could be asked for until the
first button was pressed again.) Which face it went down on is asked of every face that is
in the photo's area, whether or not the panel shows the outlines, and **kept by what the
face is, not by its number in the list** (`viewer_faces::On`): a menu's items are built
again in every frame from the photo as the library last said it is, and the faces can be
another list by then. A face that is no longer what the menu was opened on - named since,
or gone - offers nothing: not the photo's own "Not …" items in its place, which were not
what was asked for. **The menu is of the photo on screen, as the tile's is of the
selection** (`MenuOf::Viewer`): its picks act on the photo its items were just built from,
and with nothing said of the photo - one that is gone, a viewer that is shut - it has no
items and closes. "Not Anna" is `Write::PersonRemove` with the one photo, the grid's
Remove; "Locate in photon" shuts the viewer and finds the photo in All photos, or **in
Hidden when the library says it is hidden** (`App::locate`'s `hidden`), the one view that
holds it then; "Reveal in file manager" and "Open in default app" ask the system
(`app/open.rs`), as the tile menu's do. The menu is dark in both themes, as the viewer is (`MenuView::show_in`) -
seen in a picture, where it was light in a light window; the frame test holds `show_in`,
and nothing holds that the application calls it so but the picture.

**A menu or a dialog asked for in a frame that stepped is not opened** (`show_viewer`'s
end: only while the library has said what the photo on screen is, which after a step it
has not). A held arrow key repeats thirty times a second and a wheel is turned while a
button goes down. Opened, the menu stood over the next photo and - by a race between the
library's answer and the next frame - closed or became that photo's; and "Who is this?"
stood over the next photo asking about a face of the one that was left, with a face of the
next one's own in the same place: the name typed was written to a photo nobody was looking
at. It is the rule the panel's writes have ("to the photo the panel was drawn for"), met a
third time. The same test keeps both from opening over a viewer the frame has closed.

**The viewer is deaf under a dialog and under its own menu** (`ViewerData::paused`). It
reads the wheel and the buttons from the frame's events, and a dialog takes only the keys,
what is typed and a button going down out of the frame, a menu only the keys: under either
the wheel steps to no photo, a held press is over, and what the wheel had turned is not
kept for after. A press beside the menu closes it before the viewer is drawn, and is then
the viewer's, as on the grid. What the viewer answers in a frame drawn under a dialog is
dropped, as the shell's is, and by the same word (`under_dialog`, read before the dialog
is shown, or a dialog opened since): **the frame a dialog opens in and the frame it closes
in are the dialog's** - a notch of the wheel in the frame the menu's "Name this face…" is
picked would step under the dialog it opens. Each of the two - deaf, and dropped - has its
own test, and needed one: the first test of "nothing reaches the viewer under its dialog"
passed with either taken out, each hiding that the other was gone - the wheel's step is
dropped with the answers, a click on the star in the dialog's first frame is not read by a
viewer that is deaf. What tells them apart is the wheel with Ctrl, which zooms and answers
nothing, and that click, which egui hands to the star whatever the viewer reads. **And
egui's walk of the keyboard is not called off in the viewer under a dialog**: the viewer
calls it off for itself after the dialog is drawn. Tab egui has settled by then, among the
dialog's own widgets; what was called off was an arrow's walk from one of the dialog's
buttons to the next - a person's row, reached by Tab - which egui makes at the end of the
frame. (The comment first said it was Shift+Tab wrapping round the dialog's end, and a
probe passed: the test of that passed with the rule gone.)

**The person dialog is the fourth dialog, and it is the keyword picker's drawing**
(`person_picker.rs`, `createPersonPicker` and the lines of `people.ts` it says;
`dialog_view.rs`'s `Picker`: a title, a field with its button, a list narrowed by what is
typed - and for this one a line under the field that says what Enter will do). What it
names is taken as it opens: a face by its id - the outline pressed, or the menu's "Name
this face…" - or the photos selected, for the grid's "Add N photos to a person…", which
works now. The people it lists are `named_people`, read by a task as it opens (`App::named`,
forgotten at each opening): **until they are read none is listed, nothing is said in the
list's place, and the line under the field says what it says at rest** - "No one is named
yet." would be said, for the moment the read takes, of a library with fifty people in it,
and by an empty list every name typed is "a new person". A read that fails leaves them not
known, for the same reason; a name can be typed all the same. **A dialog that is a field
over a list stands where it would at its tallest and grows downwards** (`Picker::tallest`):
it is as tall as its list, which is another list at every letter and - the people's - is
not there at all until a moment after the dialog opens, and in the middle of the window by
its own height its field moved a hundred points under a name being typed. That is the
export dialog's lesson (above) met in the keyword picker, which had moved as its list
narrowed since it was built. A name is written as it is typed, on the queue of writes
(`Write::Name`): the library is what knows whose it is, whatever its case. **What is said
afterwards is the library's**: "This is Anna." by the name the person has, which the
write's worker reads after the write (where the Svelte picker goes by the list it read as
it opened); "That face is no longer there." when `name_faces` answers no one, and no
person is made of the name; and for photos `named_items_message`, `people.ts`'s word for
word. Nothing here tells the viewer or the lists: the rebuild the engine makes after a
write of the people is what has the photo read again, and a name written reloads no
picture. **The dialog goes with the viewer** (`shut_viewer`): shut from under it by a grid
of other results that does not hold the photo, the viewer left "Who is this?" standing
over the grid. And **an outline's click a moment after a dialog closed or a menu's item
was picked asks nothing** (`App::overlay_closed`, egui's double-click time): a double
click on a person's row names at its first click, and its second landed on whatever the
dialog had covered - another face's outline - and asked about that; a double click on
"Hide photo" over an outline did the same. (`DialogView::deaf` is the same double click
the other way round, and the Svelte viewer's `menuClosedAt` the same rule for its zoom.)

Three things about the tests. A dialog's title and a face's plate tell AccessKit their
words, which is how a test of the whole application reads them - so a test that presses a
person's link in the panel asks for the *link* of that name
(`get_by_role_and_label(Role::Link, ..)`): the plate over the photo says it too. The
faces of a test's library are written through the library's own calls before the launch
(`looked` in `tests/faces.rs`: `write_face_batch` for every photo, so the launch's pass
finds none to detect); a picture's are written after it, through a second connection
(`shot_over`), because the pictures' library is one the application has scanned itself -
rectangles placed for the picture, as the screenshot mock's were, one of them on the skyline. And
**a read can be made to fail**, which the plan had said it could not: a second connection
renames the table out from under it (`tests/faces_edges.rs`, which holds what a review
found). Not tested: a Mac's Control with the first button on an outline, which asks for
the menu and whose click is the outline's as well - the view answers a menu asked for in
the frame of an outline's click alone, and the click of a later frame finds the viewer
deaf under the menu, but that the two cover a Mac's press has not been run on one; and that
the people's list is forgotten at each opening (no seam holds the read back).

**A turn and a crop are the engine's edit, written from the viewer and never drawn by it**
(spec "Turns and the crop"; `crop.rs`, `crop_tool.rs` and `crop_view.rs` - `crop.ts`,
`createCropTool` and the Svelte viewer's crop markup). `R` and Shift+R are
`commands::rotate_item` and "Original" is `set_item_edit` with no turns and no crop,
`Write::Turn` and `Write::Edit` on the queue of writes; the engine rebuilds the grid, whose
key for the photo is another, `Viewer::follow` takes it, and the picture is asked for under
it at once. **A round trip later `took` hears the same from the library, and asks again
only for a picture changed under the key it had** (turned and turned back between two
frames), compared with the `Shown` it replaces and not with the viewer's key, which
`follow` has already moved: compared with that, every edit made its picture twice
(`a_turn_in_the_viewer_has_its_picture_made_once`, in `tests/edits_edges.rs` with the rest
of what the review found).
No photo is written: `tests/edits.rs` reads the watched folder back byte for byte after
every write it makes. **A turn is of the photo on screen**, read in the frame's action loop
like the star - so an arrow and `R` in one frame turn nothing (nothing is known of the photo
stepped to), and `R` then an arrow turns the photo that was seen. Each key at its press, not
its repeat: a held `R` would be a write and a render thirty times a second. **Whether the
photo can be edited** is the Svelte viewer's `editable`: the library has said what it is
(nothing is known of a photo that is gone), it is not a video, orphaned, unshowable, or a
photo whose full picture could not be made.
The three tools stay in their places, fainter and deaf, where it cannot; a video's bar has no
tools for it at all, as in the Svelte bar. "Original" is there only while the photo has an
edit, between the crop's tool and the info panel's, and widens the bar - and stays while
nothing is known of a photo stepped to (`ViewerView::edited`), or the buttons after it
jumped at every step through edited photos, the jump `bar_place`'s `was` ends for the bar.

**The crop tool is the viewer's while it is open** (`Viewing::crop`, a `Cropping`), drawn by
`ViewerView::cropping` in place of everything but the button that closes: no buttons at the
sides, no slider, no caption, no faces, no panel. It opens on the whole window with the info
panel shut and the zoom put back, on the crop the photo has, free. **It closes when its
photo can no longer be edited or is another picture**: `if !editable` in `show_viewer` -
gone, left the view, stepped from, picture refused, which covers a step made in the frame
the tool opened in - or the grid's key for the photo is no longer the one it opened under
(`Cropping::key`: turned or rewritten from elsewhere, its own "Apply" landing), which is a
round trip before the library says so, and in the frames between it asked for a picture of
the new state that nobody would draw; and when the library says the picture changed under
the key it had (`took`). **A press within a double click's time of the tool's first frame
presses nothing on its bar until it has come up** (`crop_deaf`, as `DialogView::deaf`): the
tool's bar stands where the viewer's did, and the second click of a double click on the crop
button landed on "Whole photo". **The keyboard is the
tool's**: Enter applies and Escape cancels, each at its press, and no other key is answered;
the wheel does nothing, the other button asks for no menu, the mouse's back button still
closes the viewer, as the cross does. The tool's bar is drawn **before** its keys are read,
because its dropdown (`select_view`, `above: true`, `palette: Some(&DARK)` - the viewer's
glass is dark in both themes) takes out of the frame the Enter that chose a ratio and the
Escape that shut the list, which are not the tool's as well. **The rectangle's box is the
size the library gives** (`uncropped_width`, `uncropped_height`) fitted to the window, never
a texture's: for a photo that has a crop, the picture on hand is of the part the crop keeps,
and is drawn in that part of the box (`Cropped::part`) until the picture turned and not
cropped has been made for the tool (`PictureAsk::uncropped`, `Cropping::uncropped`) - the
Svelte tool draws nothing until its image loads. That picture is the one question asked
while the tool is open on such a photo, after the viewer's rest like any other; a photo
with no crop has it already and none is asked. Made, it is the tool's and nobody's once the tool closes; refused, why is
said and it is not asked again, and the tool stands on the stand-in. **"Apply" closes the
tool when the library has stored the rectangle** (with the turns the photo has), as the
Svelte tool does: until then the tool takes no drag, no ratio and no second "Apply" (`sent`;
"Cancel" works, and the write lands all the same); refused,
it stands to be put right and takes "Apply" again; and the answer closes only the tool that
sent it (`Cropping::number`) - one cancelled and opened again while the write was on its
way is another (`an_answer_closes_only_the_tool_that_sent_it`, which writes the crop the
photo has so that nothing else could close it). A ratio chosen on a small rectangle is
grown back to `MIN_SIDE` (`fit_aspect`): on a panorama held to 1:1 it shrank a side below the
least crop the library takes, and "Apply" was refused - as it is in `crop.ts`. Known and
left, as in the Svelte viewer: a
turn on its way when "Apply" is pressed is overwritten by the turns the tool read - which
needs `R`, `C` and Enter inside one round trip.

**The slideshow is `slideshow.rs` and `slideshow_order.rs`, and its countdown is the
photo's** (`createSlideshow`, `slideshow-order.ts`, with their tests' cases). It starts
when the photo's *full picture* is shown, or cannot be (refused, unshowable, gone) -
never at the preview, and never at the frame's word for a photo an arrow in the same frame
stepped from (`start_show`) - and a step forgets it (`App::left`, the one place every way
of going on to another photo goes through). `Slideshow::shown` counts once per photo,
because the frame asks it in every frame, and `due` leaves the step to the caller: a view
of one photo has nowhere to go and the show then stands still, as the Svelte show does.
The show's own steps, by its countdown or by hand, follow its order (`show_step`: the next
photo that is not a video, by the stride, wrapping), and do not wait for the rest a step by
hand waits for. **From a photo the view has lost - hidden under the show, gone - the walk
begins a place back**: whatever stands at its offset is the photo that came after it, as
`Viewer::step` has it; from the offset itself the show skipped that photo, and in a view of
two stood on the hidden one for good. A shuffled show's stride is drawn by the standard library's seeded hasher
(`random`), and kept across a view that changes length wherever it still fits. The
interval and the shuffle are read once at each start, off the UI thread
(`App::show_settings`; a failed read plays four seconds in order). **A show between two
photos is a still window**: frames are asked for at the countdown's end, when the controls
go (`Slideshow::next`), while the photo left fades, and once at the end of a step's rest
(which falls inside the fade), at no other time - which a test
holds over a library on a drive that is not there, a watched folder's own work asking for
frames of its own every few seconds. The photo left is the picture the last frame drew at
fit (`ViewerView::drawn`), kept as a texture and its rectangle and drawn over the one
arriving, fading over 600 ms from when that one is shown. **The window goes fullscreen as
the show starts and is given back as it stops, only if the show took it there**: `start`
and `stop` answer whether to ask (`ViewportCommand::Fullscreen`), and whether the window is
fullscreen is what it was last asked until it has done it, then what it says
(`App::window_fullscreen`, `fullscreen_pending`) - egui reports what the window last said,
a compositor says it a configure later, and a show stopped and started again at once took
the window for one the user had made fullscreen, and played in a window. Every way the viewer shuts goes through `shut_viewer`,
which stops a show first. **F11 is the window's own fullscreen everywhere**, read at the
very start of the frame, before a menu, a dialog or a held band takes the keys, and at its
press only: held, it flickered the window at every repeat. Tests play the window: `fullscreen_asked` is what the application last asked,
and `tests/slideshow.rs`'s `obey` sets the viewport's `fullscreen` to it, which
`egui_kittest` keeps from frame to frame. Escape stops a show before it closes the viewer;
`S` starts and stops, Space pauses; the show is held while a dialog or the viewer's menu
is over it and while a keyword is typed in the info panel - asked before the countdown is,
so a menu asked for in the frame before its end holds it - and the crop tool stops it. **The
controls at rest are woken by the pointer moving and nothing else**, as in the Svelte
viewer: woken by a press, the second click of a double click on the photo where the cross
had been pressed the cross it woke.

**Compare is `compare.rs` and `compare_view.rs`, and its zoom is the viewer's own**
(`createCompare`, `differingFacts`, `Compare.svelte`): one `Zoom` for every pane, measured in a
pane's size, so the point a zoom holds still and the pan's bounds are `viewer_zoom.rs`'s and
tested there; the wheel, a pinch (`Event::Zoom`) and a drag move them all. It opens from `C`
in the grid (`Ask::Compare` - not under a held press, as Enter is not: the button came up
where the grid never heard it, and a band was drawn with no button down) and the tile's
"Compare N photos", for two to four photos, **once the library has said what each is**
(`App::comparing`, off the UI thread): half a comparison is worse than none. **It goes up
at the end of the frame its answer arrives in, and only over the grid** (`compare_landed`):
a key pressed before compare was ever drawn is the grid's, and a viewer, a menu, a dialog or
a held band come since it was asked for keep the window - the answer is dropped, `C` being
there to press again. It takes the whole window as the viewer does - drawn instead of the
shell, Tab called off (a message's button over the panes would take the keyboard), its keys
read from the frame's events, and a press on anything lying over a pane (a message) not the
pane's. Panes stand in two columns, three leaving the fourth place empty so every photo is
one size. Fitted, a pane is drawn from its preview - twice or three times a pane in a window
of ordinary size, from one level; **once the zoom has left the fit every pane's full picture
is made, one after another, the pane with the keys first** (`compare_pictures`, no fitted
copy: a box of nought makes none), with no level past twice a pane until a zoom outgrows
them, as the viewer's are fitted. **A pane follows its photo**: its star and its key from
the grid on screen where that holds it - a photo turned elsewhere has its full picture made
again under its new key - and its star from what was written (`Wrote::Stars`' ids), since in
Starred a photo unstarred from compare leaves the grid and its pane would say "starred" for
good, and `S` would unstar it again. Enter opens the pane with the keys in the viewer when
the grid on screen holds the photo (the viewer goes through the grid's photos), and
otherwise does nothing. While a drag is held the keys are its: Escape lets go of it and
closes nothing. What a pane says beneath it - and to AccessKit, with its star - is only what
the panes disagree about, on every pane.

