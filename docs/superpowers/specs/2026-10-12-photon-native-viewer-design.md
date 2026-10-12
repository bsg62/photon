# The native UI: the viewer

Date: 2026-10-12. Sub-project 4 of `2026-10-09-photon-native-ui-design.md`. It is built on
`native-ui`, after sub-project 3 (`2026-10-11-photon-native-grid-interaction-design.md`),
whose six pull requests (#181 to #186) are merged.

## What this is for

After sub-project 3 `photon-native` is a grid to work in, and a photo cannot be looked at:
Enter and a double click do nothing. This sub-project is the viewer. A photo is opened from
the grid and shown at full size, with the ones beside it a key away; it is zoomed and
panned; its information stands beside it; its faces are outlined and named; it is turned
and cropped; a view is played as a slideshow; two to four photos are compared.

It is also the first sub-project in which the native window draws a picture the web view
used to make: until now every pixel on screen was a thumbnail photon had rendered. An
unedited photo in today's viewer is the file itself, decoded, scaled and colour-managed by
the web view. Natively photon decodes it, scales it and converts its colours, and how it
does each is this spec's to say.

No new kind of write is made. A turn and a crop are the `edit` the engine already stores; a
name is the `people` data it already keeps; a keyword and an album are sub-project 3's
writes from another place. No photo file is opened for writing.

## Decided with the user (2026-10-12)

- **One spec, eight pull requests** ("The eight pull requests", below).
- **Colour profiles are converted in the viewer only.** Thumbnails, and what an export
  re-encodes, go on ignoring a profile, as they do today. The conversion is moxcms, pure
  Rust and already in the lockfile. (One point of this was turned round while writing: the
  first item of the next section.)
- **An animated GIF or WebP is a still frame** in the native viewer until sub-project 5,
  where playing its frames comes with video.
- **The slideshow goes fullscreen now, and so does F11.** Remembering fullscreen across
  launches waits for sub-project 7's window state.

Standing from sub-projects 2 and 3: writes are on from the pull request that builds them;
what needs a later sub-project is drawn in place and takes no press; English numbers and
dates; one fresh reviewer for each pull request, and merged as it lands.

## Decided while writing

Each is a choice made here so that the spec is whole. They were put to the user with the
spec on 2026-10-12, the first marked as departing from an answer given, and the answer was
to go ahead. Any of them can still be turned round before the plan of the pull request it
is in.

1. **An edited photo's colours are converted too.** The answer chosen was "the viewer
   only", against "the viewer and edited photos"; the difference between them exists in
   today's photon because an edited photo is served to the web view as a JPEG photon
   re-encoded without its profile. Natively there is no such second road: the viewer
   decodes the file and applies the edit to the pixels it has, so an edited photo is
   converted by the same line as an unedited one, and *not* converting it would take a
   line of its own whose only effect is that a photo's colours go flat when it is turned.
   Thumbnails and exports are untouched either way, which is what the answer was about.
2. **The viewer takes the whole window.** The top bar, the sidebar and the status bar are
   not drawn under it, as the Svelte viewer covers them. The grid keeps its place, being a
   number of the application's and not a widget's; a name half typed in the sidebar is
   stored when the viewer opens, as it is when the sidebar is hidden.
3. **Full size is held for one photo, and asked for after a rest.** The photo on screen
   and nothing else is decoded at full size; its neighbours are warmed as previews, which
   are on disk already. A step by hand waits 130 ms before the decode is asked for, as the
   Svelte viewer waits before it fetches: a decode cannot be taken back once begun.
   (Measured for this spec, release build, generated photos: 24 MP in 60 ms, 45 MP in
   113 ms, and 96 and 182 MB of pixels. A camera's JPEG is slower than a generated one;
   the swap from preview to full is still well under half a second.)
4. **A video opens as its poster frame**, with one line that says the native window does
   not play videos yet. Its controls are sub-project 5's and are not drawn. The slideshow
   skips videos, as it does today.
5. **Drawn in place and inactive**: "Copy photo" and Ctrl+C, "Reveal in file manager",
   "Open in default app", "Open in OpenStreetMap" and the click that copies the file's
   name (sub-project 7: the clipboard and the system's openers); "Show N duplicates in
   the grid" (sub-project 6). The `?` sheet is sub-project 6's and has no key yet.
6. **The wheel steps one photo a notch**, whatever Windows' lines a notch are set to: the
   setting says how far a page scrolls, and here nothing scrolls. A trackpad's stream adds
   up to a notch's worth before it steps, as today. With Ctrl or Cmd it zooms.
7. **Two changes outside `photon-ui`, neither of which the Tauri photon calls**: the
   engine gains the command that renders the viewer's picture, and photon-core the reading
   of an embedded profile and its conversion. Nothing existing changes behaviour.
8. **F11 works in the grid as well as in the viewer**, since it is one line once the
   slideshow has fullscreen.

## What was read first

- `ui/src/components/Viewer.svelte` (1,741 lines) and what it stands on: `viewer-layout.ts`,
  `picture.ts`, `full-load.ts`, the zoom half of `nav.ts`, `slideshow.svelte.ts`,
  `slideshow-order.ts`, `crop.ts`, `crop-tool.svelte.ts`, `faces.ts`, `histogram.ts`,
  `caption.ts`, `exif.ts`, `compare.svelte.ts` and `Compare.svelte`, `PersonPicker.svelte`.
- `commands::viewer_item` and `ViewerItem`: everything the viewer says about a photo is in
  that one answer - the facts, the faces as shown, the edit, the dates read from the file,
  the position, the histogram (counted by the engine from the grid thumbnail), the copies.
  It reads SQLite and the file, so it is asked off the UI thread.
- `protocol.rs`'s `/image/<id>`: the original file for an unedited photo, and for an edited
  one `edit::render_picture` under `RENDERING`, re-encoded. `render_picture(path,
  orientation, edit)` is "the photo as shown: decoded at full resolution, turned upright,
  then turned and cropped by its edit", and is what the native viewer draws from.
- `egui-wgpu` 0.36.2, `renderer.rs`: **a texture has one level** (`mip_level_count: 1`),
  whatever `TextureOptions::mipmap_mode` says. A 24 MP texture drawn into a 2,700-pixel
  area is sampled from four pixels in every ten: it shimmers and crawls. The web view
  scales with a proper filter, so this is a thing photon has to do for itself.
- `egui` 0.36.2: `InputState::max_texture_side` is the device's limit, handed over by
  eframe; `ViewportCommand::Fullscreen`; `zoom_delta` is what a Ctrl+wheel and a pinch
  come to; `PointerButton::Extra1` is the mouse's back button.
- `image` 0.25.10: `ImageDecoder::icc_profile` answers for JPEG, PNG, TIFF and WebP.
  `moxcms` 0.8.1 is in `Cargo.lock` already, as `image`'s own dependency.

## What is in it

### The viewer's state: `viewer.rs`

A state module, with no egui type in it. What is open: the photo's id and its offset in
the grid on screen; what the library last said of it (`ViewerItem`); whether it has left
the view (`orphaned`) or the library (`gone`); the zoom and the pan; whether the info panel
is open. `App::viewer` is `Some` while the viewer is open.

**Most of the Svelte viewer's guards have no successor**, for the reason most of the Svelte
selection's had none: this window holds the whole `GridIndex`, so "which photo is at this
offset" and "where is this photo now" are read in the frame that asks.

- **A new grid is followed by id** (`Viewer::follow(old, new)`, called where
  `Selection::follow` is). The photo found in the new index is at its new offset and
  nothing else happens: no reload, since the loader goes by the photo and its key, never
  by an offset. `createLoadSlot`, `rebind`, `renumbered` and the `detailsSeq` counter
  exist to tell a renumbering from a step across a round trip, and are not ported.
- A photo the new grid does not hold has left the view or the library; only the engine can
  say which. `viewer_item` is asked again: an answer makes it `orphaned` (it stays on
  screen, with no position in its caption, and the next step goes on from where it was -
  after a star taken off in Starred, a hide, a "Not Anna" in Anna's view); a refusal makes
  it `gone`, and the viewer says "This photo is no longer available."
- **What reloads the picture is `picture_changed`**, ported with its reasons and its
  tests: the thumbnail key, the dimensions, the orientation, and the thumbnail state only
  across `failed`. The faces are not in what it compares. A reload starts the photo fitted
  and closes the crop tool, so the comparison has to stay exact.
- A step to another photo resets the zoom and the pan, as today.

`viewer_item` is asked through a `tasks::Latest` - at a step, and at every change of the
library while the viewer is open - and its answer counts for the photo on screen only.

### The picture

Two layers, as today: **the cached preview at once**, and the full picture over it when it
is ready. The preview is the 1600-pixel thumbnail the grid's loader already knows how to
ask for; a photo opened from the grid has it on disk.

**The full picture is made by the engine, off the UI thread**: a new command,
`commands::viewer_picture(engine, id, uncropped)`, looks the photo up, takes `RENDERING`
(as `/image` does for an edited photo, so that an export and the viewer do not decode at
once), calls `render_picture`, converts the colours (below) and answers RGBA pixels with
the thumbnail key they are of. The viewer's own loader - one thread, one question at a
time, the latest wins - asks it. An answer whose key is not the one the photo has now is
dropped: the picture changed while it was being made.

**photon makes its own smaller levels.** The loader halves the picture, by the mean of
each four pixels, until a level fits the photo's area, and hands all of them over; the
view draws the smallest level that is at least as large as what is drawn, so no texture is
ever drawn at less than half its size and a plain linear filter is enough. A third more
memory than the full picture alone. Each level is a texture of egui's.

**The device's limit** (`max_texture_side`): a level larger than it is not made into a
texture, so the deepest zoom into a very large panorama is the first half that fits - the
umbrella spec's fifth risk, as decided there. The zoom's own limit stays four times the
fitted size.

**Uploading 96 MB in one frame** is a long frame at the moment the sharp picture arrives.
The first pull request measures it; if it is more than a frame, the full level is
uploaded in strips over several (`TextureHandle::set_partial`), smallest level first, so
that the photo is sharp at fit at once and sharp when zoomed a moment later.

**What is let go.** The textures of the photo that was left are freed when the next
photo's preview is on screen; nothing of full size is kept for a photo not on screen. The
previews of the two photos on either side are warmed after a rest, as the Svelte viewer
preloads its neighbours, by the engine's `neighbours` (which leaves edited photos out).

**What cannot be shown** says so in the photo's place: a photo whose thumbnail failed
("This photo can't be shown.", or the engine's reason), a decode that fails, a photo that
is gone. A video is its poster and its one line.

### Colour

`photon_core::colour` (new): `embedded_profile(path)` reads the file's ICC profile where
its decoder gives one (JPEG, PNG, TIFF, WebP), and `to_srgb(pixels, profile)` converts
RGBA pixels with moxcms. `viewer_picture` calls both for the picture it has rendered - a
conversion goes pixel by pixel, so it is the same before a turn or a crop as after.
Nothing else calls them.

- **RGB profiles only.** A profile of another colour space (a CMYK JPEG's, a grey one) is
  not applied: the decoder has already made RGB of the pixels by its own rule, and the
  profile no longer describes them.
- A profile moxcms refuses, or one that is sRGB already, costs nothing and changes
  nothing. A photo with no profile is taken to be sRGB, as a web view takes it.
- **AVIF**: converted where the container carries an ICC profile. One that says its
  colours by numbers instead (`nclx`) is read in the fourth pull request's plan, against
  what `zenavif-parse` hands over; the floor is that such a photo is shown as it is today
  in an edited render, unconverted.
- **The preview is not converted** - thumbnails are not - so a wide-gamut photo changes
  colour a little when its full picture arrives. Today's viewer does the same.

### The view: `viewer_view.rs`

Drawn in place of the shell while `App::viewer` is some: a black ground in both themes -
a photo is judged against black - with the dark palette for everything on it. The layout
is `viewer-layout.ts`'s, in a state module of its own (`viewer_layout.rs`): the photo's
area is the window, less the info panel's room while it is open.

- **The bar**: the star, turn left and right, crop, "Original" while there is an edit,
  the slideshow, the info panel, the zoom's slider, and close in the corner. Previous and
  next at the window's sides, which take no press at the ends of the view and are not
  dimmed out of the Tab order.
- **The caption line**: the file's name, when it was taken, its size in pixels and in
  bytes, and its position in its section (`formatCaption`, `positionInSection`), and the
  photo's own caption under it where it has one.
- Every control tells AccessKit what it is, as everywhere, which is how a test of the
  whole application presses it.

### The keys and the mouse

The viewer's keys are the Svelte viewer's (`shortcuts.ts`, the viewer's group): the arrows
for the photo before and after, Home and End, Escape and Backspace and the mouse's back
button to close, `.` for the star, `H` to hide and again to bring back, `+` `=` `-` `0`
for the zoom, `I` for the panel, `R` and Shift+R to turn, `C` to crop, `S` for the
slideshow. A menu or a dialog over the viewer takes its keys first, as over the grid; the
crop tool owns the keyboard while it is open (Enter applies, Escape cancels, nothing else
leaves the photo).

**The grid is not drawn under the viewer, so its keys cannot be answered twice** - the
gate sub-project 3 built for the grid's keys is not needed here, and the viewer has one
place that reads a key (`viewer_view.rs`), after the menu and the dialog.

- **Zoom** is `nav.ts`'s arithmetic, ported with its tests: `zoom_at` holds the point
  under the pointer still, `clamp_pan` keeps the photo over its area, a quarter a key
  press, a double click to the photo's own pixels and a second one back
  (`actual_size_zoom`, by the device's pixels a point). Measured from the middle of the
  photo's area, not of the window, so that it is right beside the open panel.
- **Pan** is a drag with the first button while zoomed. Its press is read from the
  button's own events, as the grid's band is, with the endings a band has: the button up,
  the window left, a touch cancelled.
- **Closing** hands the photo back to the grid: it is selected, alone, and brought into
  sight. An orphaned or gone photo hands back none.

### Opening it

Enter on the lead and a double click on a photo open the viewer on that photo - the two
things sub-project 3 left doing nothing. A double click is the second click of two on one
photo within egui's double-click time; the first has selected it.

### The info panel

Beside the photo, `INFO_WIDTH` wide: the file's name and path; the caption; the camera's
rows (make and model, lens, exposure), each a link to its search; the histogram
(`histogram.rs`: `histogramPeak` and the outline, ported with their tests); the dates;
the position, with "photos near here" as a search; the people as links to their views; the
keywords; the albums; the copies, each a link that locates it.

- **A keyword is added and taken off here** (`add_item_tag`, `remove_item_tag`) and **an
  album ticked** (`add_to_album`, `remove_from_album`): sub-project 3's writes, on the
  queue of writes. Picasa's albums are listed and cannot be ticked.
- A link that leaves the viewer - a search, a person, a locate - closes it and goes
  through `App::go`, as a sidebar row does.
- The panel scrolls when it is taller than the window.

### Faces

While the panel is open the faces are outlined over the photo: a named one with its plate,
an unnamed one as an outline that can be pressed. Their rectangles are fractions of the
picture as shown (`ViewerItem` has mapped them through the edit) and are placed by
`faces.ts`'s arithmetic, ported.

- **The viewer's menu** (the one menu, opened where the other button goes down): for the
  face under the pointer "Name this face…" or "Not *name*", elsewhere "Not *name*" once
  for each person photon has a face of here; then "Locate in photon", "Reveal in file
  manager" and "Open in default app" (inactive), and "Hide photo".
- **The person dialog** is a fourth dialog of the dialog layer (`person_picker.rs`,
  `PersonPicker.svelte`): a name typed, the people photon knows listed under it
  (`named_people`, read as it opens). It names the face it was opened for (`name_faces`),
  taken as it opens, as every dialog's subject is.
- **"Add *N photos* to a person…" in the tile's menu** is switched on with it
  (`name_items`), and what it says afterwards is the Svelte message, with the photos it
  could not decide.
- A name written does not reload the picture (`picture_changed` leaves faces out).

### Turns and the crop

`R` and Shift+R write a turn (`rotate_item`); "Original" writes no edit at all. The
picture that follows is the engine's: the write rebuilds the grid, the photo's key
changes, `picture_changed` says so, and the viewer asks for the picture again. Nothing
here draws a turn.

The crop tool is `crop.rs` and `crop_tool.rs` (state: the rectangle in fractions, the
eight handles, the ratios, `drag_rect` and `fit_aspect` with their tests) and its view.
It is drawn over **the picture turned and not cropped** (`viewer_picture` with
`uncropped`), fitted; the ratio is chosen from photon's own dropdown, opening upwards.
"Apply" writes the edit and the tool closes when the write has succeeded; "Whole photo"
is no crop; Escape cancels.

### The slideshow

`slideshow.rs`, a state module on a clock that is a number, as the search box's is:
`createSlideshow` with its tests. The countdown belongs to a photo that is on screen, not
to the clock - it starts when the full picture is shown, or has failed for good - and is
held while a dialog is over the viewer.

- **The order** is `slideshow-order.ts`, ported: the next photo that is not a video, and
  for a shuffled show a stride that shares no factor with the view's length. The interval
  and the shuffle are read once at the start (`slideshow_interval`, `slideshow_shuffle`),
  off the UI thread, and a failed read plays four seconds in order.
- **The crossfade**: the photo being left is drawn over the one arriving and faded out
  over 600 ms once the arriving one is shown. Frames are asked for while it fades and at
  the countdown's end, and at no other time: a slideshow between two photos is a still
  window.
- **The controls go** when the pointer has rested for two and a half seconds, and come
  back when it moves.
- **Fullscreen** is asked of the window as the show starts (`ViewportCommand::Fullscreen`)
  and given back as it stops, unless the window was fullscreen already. F11 does the same
  by hand, in the grid and in the viewer.

### Compare

`C` in the grid and "Compare *N photos*" in the tile's menu, for two to four selected
photos: `compare.rs` (state: the panes, the one zoom and the one pan that all of them
share, which pane has the keys) and its view. Each pane is drawn as the viewer draws a
photo - preview, then the full picture with its levels - by a loader that holds as many
pictures as there are panes. What the panes disagree about is said on each
(`differing_facts`). The keys are today's: `1` to `4` and Tab for the pane, `S` and `.`
for its star, Enter to open it in the viewer, Escape back to the grid.

Four full pictures are four times one photo's memory (about half a gigabyte for four of
24 MP). Compare asks for a pane's full picture only once the zoom leaves fit; fitted,
each pane is its preview, which is about as large as a pane is.

## The eight pull requests

1. **Open, look, step, close.** `viewer.rs` and its `follow`; the engine's
   `viewer_picture`; the loader with its levels and the device's limit; the view with its
   bar, caption and messages; the star and the hide; the keys and the wheel that step and
   close; Enter and the double click in the grid; the photo handed back. The upload's
   cost measured.
2. **Zoom and pan.** The arithmetic, the slider, the keys, the wheel with Ctrl, the double
   click, the drag.
3. **The info panel**, the histogram, the keywords and the albums in it, its links.
4. **Colour.** `photon_core::colour`, the conversion in `viewer_picture`, AVIF's plan.
5. **Faces.** The outlines and plates, the viewer's menu, the person dialog, "Add to a
   person…" in the grid.
6. **Turns and the crop.**
7. **The slideshow**, fullscreen and F11.
8. **Compare.**

Each is opened into `native-ui`, read by one fresh reviewer, fixed in one pass and merged.

## What is not in it

- Playing a video, and playing an animated GIF or WebP: sub-project 5.
- The People page, Settings (the slideshow's interval and shuffle are read, and set only
  in the Tauri photon until then), Statistics, the shortcut sheet, duplicates: sub-project
  6.
- The clipboard, the system's openers, the file dialogs, window state: sub-project 7.
- Colour conversion of thumbnails and exports: not decided, and not this sub-project's.
- Writing a caption: nothing in photon writes one.

## Tests

- **State modules** with the Svelte tests' cases, ported: `viewer.rs` (`follow`,
  `orphaned`, `picture_changed`, the position), `viewer_layout.rs`, the zoom arithmetic,
  `histogram.rs`, `crop.rs` and `crop_tool.rs`, `slideshow.rs` and its order,
  `compare.rs`, `person_picker.rs`. Each new one is added to `STATE_MODULES`.
- **`photon_core::colour`**: pictures made for the test with a profile embedded - a
  Display P3 and an Adobe RGB one - whose known pixels come out as the sRGB values worked
  out by hand; a photo with no profile, an sRGB one, a broken one and a CMYK one
  unchanged.
- **The engine's `viewer_picture`**: the picture of an unedited, a turned and a cropped
  photo, each the size `ViewerItem` says; `RENDERING` held holds it; a photo that is gone
  refuses.
- **Views in whole frames**, without a window, as the dialogs' are: what is drawn where,
  what a press answers, which level is drawn at which zoom.
- **The whole application**, through `Driver`, over libraries of real photos in a
  temporary directory: opened by Enter and by a double click; stepped by the keys and the
  wheel; closed onto the photo; a star and a hide and what `orphaned` then does; a keyword
  written from the panel; a face named; a photo turned and cropped and the picture that
  follows; a slideshow's countdown on the driver's clock; a still window in each state
  that should be one (a photo shown, the panel open, a slideshow between two photos).
- **Probes**, as in every pull request so far: each rule broken in turn, with the test
  that is to fail. A probe that passes is a finding.
- **Pictures** (`native-shot`): the viewer, the panel, the faces, the crop tool, compare,
  in the dark ground both themes share.

## Risks particular to this sub-project

1. **Scaling quality.** photon's own levels and a linear filter are not a web view's
   resampling. At fit a level is drawn between half and all of its size, which is where a
   linear filter is at its best; whether that is as sharp to the eye as today's viewer is
   judged in a picture of a detailed photo beside the Svelte one, in the first pull
   request, before anything is built on it. The fallback is one level resized exactly to
   the area with a better filter.
2. **Memory.** A 45 MP photo is 182 MB of pixels, a third more with its levels, and for a
   moment both in memory and on the GPU. One photo at full size at a time keeps it there.
3. **The window's own colour space.** The conversion's target is sRGB, and the window's
   surface is what wgpu makes it. On a wide-gamut display whether the system takes that
   surface as sRGB is not something a test here can see: it is on the smoke checklist for
   a wide-gamut Mac, and until someone has looked, "converted" means "converted to sRGB
   numbers".
4. **The first frames of a dialog over the viewer**, the double click that opens it, a key
   held across its opening: what three reviews found in sub-project 3 is asked of every
   surface here before its own review.
5. **A decode that cannot be taken back.** The rest before the decode keeps a held arrow
   key from queueing one for every photo passed; one decode can still be running when the
   user has moved on, and its answer is dropped by its key.

## Not decided here

- How an AVIF's `nclx` colours are read: the fourth pull request's plan.
- Whether the upload is made in strips: the first pull request, by measurement.
- Whether thumbnails and exports should convert colours too: a decision about photon-core
  and the Tauri photon, for after this sub-project.
