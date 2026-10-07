# photon

A fast, local photo manager for Linux, macOS and Windows. A spiritual successor to Picasa 3.

photon watches folders in place. It never moves or changes your photos. It keeps a small
SQLite library and a thumbnail cache in your user data and cache directories. Stars are
shared with Picasa: photon reads them from Picasa's `.picasa.ini` beside your photos, and a
star you set in photon is written back into that same file, so both programs agree.

## Install

Download the installer for your system from the [latest release](https://github.com/bsg62/photon/releases/latest).

| System | File | Install |
|---|---|---|
| Linux | `.AppImage` | `chmod +x photon_*.AppImage && ./photon_*.AppImage` |
| Linux (Debian, Ubuntu) | `.deb` | `sudo apt install ./photon_*.deb` — pulls in webkit2gtk-4.1 and libsoup-3 |
| macOS | `.dmg` | Open it and drag photon to Applications. Take the `aarch64` file for Apple Silicon, `x64` for Intel. |
| Windows | `.msi` | Run it. A desktop shortcut is made only if you tick the box on the install-folder page (or run `msiexec /i photon_*.msi DESKTOPSHORTCUT=1`); the Start menu entry is always there. |

### Upgrading to a version with Starred photos

photon reads star ratings from Picasa's per-directory `.picasa.ini` / `Picasa.ini`, applied
to each folder after it is scanned. A library built by a v0.2.0–v0.3.x version holds
ratings from that era's XMP-based source, which no INI has confirmed.

Nothing needs to be done: a normal rescan corrects a folder's stars the first time it's
walked again, whether that scan is manual or triggered by the file watcher. Deleting the
library (`io.github.bsg62.photon/library.db`, in your user data directory) is not required — it only forces
every folder to be reached, and so corrected, in one pass instead of over time as folders
are scanned. Your photos are untouched either way, since photon never writes a photo file.

### Stars and Picasa

The star button in the viewer writes into the folder's `.picasa.ini` — the same file Picasa
reads — so a star set in photon shows in Picasa and the other way round. photon changes only
the `star=` line for that one photo and leaves the rest of the file (Picasa's face tags,
crops and edits) exactly as it was. A folder with no INI gets a new `.picasa.ini` when its
first photo is starred; an old `Picasa.ini` is edited in place. On Windows the file's
hidden attribute is kept. If Picasa has the file open at that moment the write fails with
a message and nothing changes; try again once Picasa has finished.

A star lives in that file under the photo's file name, and that goes for a star set in photon
as much as for one set in Picasa. So it follows a folder you rename or move, because the
folder's `.picasa.ini` goes with it, but not a single photo you rename or move to another
folder: the file has no line for the new name, and the photo comes back without its star. Star
it again. photon does not carry the star over by itself, because that would be writing into
your folder without being asked.

### Picasa albums

Albums you made in Picasa 3 appear in photon's **Albums** list, marked with a small
stacked-photos icon, once the folders holding their photos have been scanned. photon reads
them from the same `.picasa.ini` Picasa writes beside the photos and follows every change on
the next scan, but never changes them: renaming, deleting, adding and removing are Picasa's.
To make one editable, open it, select its photos and add them to an album of your own. An
album Picasa kept only in its own database, never written into a folder's INI, does not
appear.

### File formats

photon indexes JPEG, PNG, GIF, WebP, TIFF, BMP and AVIF. A TIFF holding several pages is shown
as its first page. AVIF is decoded by photon itself, in pure Rust, including the tiled and
10-bit photos phones write. An HDR AVIF is shown as standard range without tone mapping, so it
may look flat, and an animated one shows its still image. The viewer shows full-size AVIFs
where the system's web view can (Windows, macOS 13 and later, most Linux desktops) and the
1600-pixel preview elsewhere. Thumbnails of large JPEGs are made with libjpeg-turbo, which
decodes a photo straight at the size a thumbnail needs. Camera RAW files and HEIC are not read:
decoding them means shipping a large C library, and photon keeps its C to a few small,
vendored ones (SQLite, libwebp, libjpeg-turbo), compiled in, so it needs no system libraries
beyond the system's web view.

Adding TIFF, BMP and AVIF does not disturb a library built by an earlier photon. Those files
simply appear as each folder is walked again, whether that scan is manual or triggered by
the file watcher; nothing already indexed is re-read and no thumbnail is rebuilt.

**Video:** MP4, M4V, MOV and WebM, played by the system's own video support - photon ships
no decoder. On macOS everything an iPhone records plays. On Windows, iPhone video (HEVC)
needs Microsoft's *HEVC Video Extensions* from the Store; without it such a video shows no
preview and does not play. On Linux the `.deb` pulls in GStreamer's good and libav plugins;
everywhere else, the AppImage included, photon uses the ones the system has, so install them
(`gst-plugins-good` and `gst-libav` on Arch, `gstreamer1.0-plugins-good` and
`gstreamer1.0-libav` on Debian and Ubuntu) - without them photon shows videos but cannot play
them. The AppImage does not bundle them, and video in the AppImage has not been checked by
hand yet. A video's preview is drawn while photon's window is open.

### Camera data, keywords, people and albums

The viewer has a button at each side of the photo for the previous and the next one, beside
the arrow keys and the wheel. Its info panel stands beside the photo, not over it: while the
panel is open the photo is fitted into the room left of it, so nothing in it is covered.

The viewer's ⓘ button (or `I`) opens an info panel: camera, lens, focal length, aperture,
shutter speed and ISO from the photo's EXIF; every date the photo has - taken, digitized and
edited from its EXIF (a video's creation date), and the file's created and modified times -
with a date shared by several of them shown once; the keywords the photo carries in its XMP or
IPTC (as written by Picasa, Lightroom, Bridge, digiKam and the like); the people Picasa
named in the folder's `.picasa.ini`, outlined over the photo while the panel is open;
checkboxes for photon's albums; and the Picasa albums the photo is in. `R` and `Shift+R` turn the photo on screen; nothing is
written, and the next photo opens upright.

photon can also **find faces itself**. It is off until you switch it on, under Settings →
People → **Find faces in my photos**, and it runs on this computer only: nothing is uploaded,
and photon has no network access to upload with. It works through the library in the
background - hours on a large one, with the count in the status bar - and carries on where it
stopped after a quit. Faces it finds are outlined in the viewer while the info panel is open,
and photon groups them by likeness. Open **People** in the sidebar to name a group: its faces
then carry that name in the viewer, in the person's own view and in search, where `person:anna`
finds the photos.
Unconfirmed matches are shown there as suggestions, never as names. A face Picasa
named keeps its name and is not outlined twice, and a face Picasa marked without naming is now
outlined too. Search counts them: `has:face` is every photo with a face, `faces:2` the ones
with exactly two, `faces:3+` three or more, `-has:face` the ones with none - which, until the
first run has finished, includes photos it has not reached yet. A face lying on its side is
mostly missed, which matters only for a photo stored sideways with nothing in the file saying
so: turn it in photon and it is looked at again. Videos are not searched for faces, so
`-has:face` and `faces:0` find every video as well. Switching it
off deletes everything photon found; what Picasa recorded is untouched. The detections live in
photon's library and are never written to your photos or to `.picasa.ini`.

Under the exposure the panel draws the photo's **histogram**: how much of the picture sits
at each brightness, shadows on the left and highlights on the right. It is of the photo as
photon shows it, so a crop changes it. A tall column at either end is clipping - a blown sky,
blocked shadows - and is drawn full height without flattening the curve between. Videos have
none.

A photo's caption - what you typed under it in Picasa, or its description in Lightroom, Bridge
or digiKam - is shown under the photo in the viewer and during a slideshow, and at the top of
the info panel. photon reads it from the photo's XMP or IPTC; the text many cameras put in EXIF
("OLYMPUS DIGITAL CAMERA") is ignored. Search finds a word of it. photon does not edit
captions: that would mean writing the photo.

**All photos**, at the top of the sidebar, goes back to the whole library from Starred, Recent,
an album or a search, at the folder you were last browsing there - clicking a folder instead
opens that folder at its top. Drag the sidebar's edge to make it wider or narrower; its width,
and which of its groups are open, are remembered on this computer. The button at the left end
of the top bar, or `Ctrl+B`, hides the sidebar and gives its room to the photos, and brings it
back as you left it; that too is remembered. The sidebar lists **Albums**, **People** and **Tags** above the years. Albums are photon's
own and live only in its library: create one with "New album…", add photos from a tile's
right-click menu or the info panel, and rename or delete from the album's right-click menu.
Picasa's albums (see "Picasa albums" above) are read from its INI and cannot be edited here,
and neither can People and Tags, which are read from Picasa's INI and from the photos
themselves. The search box matches all of it: a camera or lens name, a keyword, `50mm`,
`f/1.8`, `iso400`, or a date such as `2024-06`. The **?** at the end of the box opens a list of
everything it understands, described below; click a term there to add it to your search.

Every word narrows the search: `italy lake` finds photos matching both, each word wherever
it likes (`lake.jpg` in the folder `2019 Italy`). `lake OR pond` widens it; `AND` and `OR`
count as operators only in capitals, so `salt and pepper` still looks for the word. Double
quotes make a phrase, and `camera:` or `lens:` confine a term to that field, so
`camera:canon 2019` is the Canon's photos from 2019 and not a folder named Canon. A camera or
lens in the viewer's info panel is a link to that search. `from:` and `to:` bound the date a
photo was taken, each taking a year, a month or a day and including all of it:
`from:2019-06 to:2019-08` is June to August, `to:2019` is everything up to the end of 2019.
`on:07-14` is every 14th of July, whatever the year. **On this day** in the sidebar is that
search for today's date: what you were photographing a year ago, and ten.

`tag:`, `person:`, `album:` and `folder:` work like `camera:`: `tag:zoo` is the photos carrying
that keyword and not the ones in a folder named Zoo, `person:anna` the photos Picasa named Anna
on, `album:"best of"` the photos in that album, `folder:italy` a folder by its name or the name
you gave it in photon. People and albums are found only this way; a bare `anna` does not look
at them. `is:starred`, `is:edited` (turned or cropped in photon), `is:video` and `is:photo` ask
what a photo is, `has:gps` whether it records where it was taken, and `has:face` and `faces:2+`
how many faces are on it (above). `has:tag`, `has:caption`, `has:album` and `has:person` ask
whether a photo has any at all, `is:duplicate` whether it has a copy or a look-alike (what the
Duplicates view shows), and `is:portrait`, `is:landscape` and `is:square` what shape it is as
photon shows it, turned and cropped. A hyphen in front turns any term round: `-tag:family`,
`-is:starred`, `-lake`, and `-has:tag` for the photos you have not tagged yet; in quotes it is
an ordinary hyphen. Together with saved searches this makes a collection that keeps itself:
save `person:anna is:starred -album:printed` and it is always Anna's starred photos you have
not printed yet.

`size:`, `iso:`, `aperture:`, `focal:` and `mp:` compare a number: `size:>10mb` is the files
over ten megabytes, `iso:>=1600` the high-ISO shots, `aperture:<2` the ones shot wider than
f/2, `focal:>100` the long lens, `mp:<2` the pictures under two megapixels. `>`, `>=`, `<` and
`<=` are the comparisons, and two terms make a range: `iso:>=400 iso:<=800`. With none, the
term asks for that number: `iso:400` and `aperture:1.8` exactly, `focal:50` whatever photon
shows as 50mm, and `size:10mb` or `mp:12` whatever rounds to it - no file is exactly ten
megabytes. A size needs its unit, `kb`, `mb` or `gb`. A photo that does not record the number
(a scan has no ISO) matches none of these, so `-iso:>0` finds exactly those.

### Where a photo was taken

A photo from a phone, or a camera with GPS, records its position. The info panel shows it
under **Location**, with two links. **Photos nearby** searches for everything taken within a
kilometre: the box then reads `near:46.5388,12.1373`, and you can widen it by adding a
distance, `near:46.5388,12.1373,25km`. **Open in OpenStreetMap** opens that spot on
openstreetmap.org in your browser - the only time photon sends anything about a photo
anywhere, and only when you click it. photon itself draws no map: that would mean fetching
map tiles for every photo you look at.

A library from an earlier photon picks positions up on the next scan of each folder, which
reads every file's header once, as it did for camera data. Videos' positions are not read.

### Saving a search

A search worth repeating can be kept: with a query in the box, the bookmark button at its
right saves it, and it appears under **Searches** in the sidebar. Clicking it runs the query
again — a saved search is not a collection, so a photo indexed tomorrow turns up in it
without anything being added. Right-click one to rename it (it is named after the query
until you do) or to delete it; deleting asks first and leaves the photos on screen, since
they are still the answer to what was typed. The bookmark fills once a query is saved, and
saving the same one twice does nothing.

The sidebar shows no photo count beside a saved search, unlike Albums, People and Tags.
Counting one means running it over the whole library, and doing that for every saved search
on every change would cost more than the number is worth.

Two things to know. A library from an earlier photon picks up camera data and keywords on
the next scan of each folder, which reads every file's header once; nothing needs to be
done. And a photo renamed or moved on disk inside the folders photon watches stays in its
albums; see "Renaming and moving photos" below for what is followed and what is not.

### Finding your way in the grid

The **Group by** control in the top bar, beside the sort, sets what the grid's headers are:
**folder** (one header a folder, as photon has always drawn it), **day**, **month**, **year**,
or **none**. Under a date grouping every view is one timeline across folders, newest photo
first, with a header wherever the day, month or year changes. Grouping goes with the Date taken
sort; sorted by name, size or date modified the control is dimmed and keeps its choice.

A header stays at the top of the grid for as long as you are among its photos, and the next
one pushes it out as it arrives, so deep in a folder of several hundred photos the grid still
says which folder it is. While the grid runs folder by folder, that folder also carries a small
bar beside its name in the sidebar, and the list scrolls to keep it in view - except while the
pointer is over the list, so it never moves under your hand. Right-click a folder's header in
the grid for the same menu its row in the sidebar has: Rescan, Reveal in file manager, Rename
in photon…, Hide folder.

### Photo size

The **Small / Medium / Large** control in the top bar — also under Settings → Appearance — sets
how large the grid draws your photos. Small fits roughly twice as many photos on screen as
Medium, Large makes faces and detail easier to judge at a glance. Each is the size a photo
starts from: photon widens the photos so every row runs to the edge of the grid, so the exact
size follows the width of the window. Changing the size, resizing the window or dragging the
sidebar's edge keeps your place: the photos at the top of the screen are still the photos at
the top of the screen afterwards. The choice is remembered, so photon reopens at the size you
left it, and it costs nothing to change your mind — the same thumbnails are drawn at every
size, and nothing is read from or written to your photos.

### Rotating and cropping

In the viewer, `R` and `Shift+R` (or ↻ ↺) turn a photo, and `C` (or ✂) opens the crop tool:
drag the rectangle or its handles, pick a ratio to lock it, Enter applies, Escape cancels.
Cropping again shows the whole photo with the current rectangle, so you adjust it rather than
cropping what was left. **Original** undoes everything.

None of this changes your files. photon remembers the edit in its library and applies it
wherever it shows the photo — the grid, the viewer, a slideshow — so "Reveal in file manager"
and "Open in default app" still lead to the photo exactly as the camera wrote it, and other programs do not see the
edit. An edit belongs to the photo's entry in the library, and that entry follows the file when it is
renamed or moved inside the folders photon watches, so the photo stays turned and cropped.
Picasa's own crops and rotations are not imported.

### Copying a photo

**Ctrl+C** (**⌘C** on a Mac) in the viewer copies the photo on screen to the clipboard as a
picture, ready to paste into a chat, a mail or a document; in the grid it copies the selected
photo, and **Copy photo** in a tile's right-click menu does the same. The copy is the photo as
photon shows it, turns and crops included, at most 2560 pixels on its long edge; for the
full-size file use Export or Reveal in file manager. With text selected (a caption in the info
panel, say), Ctrl+C copies the text instead. On Linux the picture is served by photon itself:
unless a clipboard manager keeps a copy, paste it before you quit photon.

### Exporting copies

Select photos, right-click and choose **Export…** to copy them into a folder you pick -
anywhere but inside a folder photon watches. A photo you have not edited is copied byte for
byte. One you have turned or cropped comes out as you see it, unless you untick **Apply edits
to the copies**. Nothing is ever overwritten: a name already there gets ` (2)`.

**Longest edge** makes the copies smaller, for a mail or a web page: choose 1920 px and
every photo larger than that is scaled down to it, while one already within it is left
exactly as it is. A scaled copy, like an edited one, is a new JPEG (or PNG, for a picture with
transparency) and carries no camera information. Videos and GIFs are never scaled. The size
goes back to **Original size** each time the dialog opens, so an export for the archive is
never shrunk by the last one's setting.

### Comparing photos

Select two to four photos in the grid and press `C` (or right-click → **Compare**) to see them
side by side — two across, or a 2×2 for three or four. Zoom and pan move every pane together,
about the same point in each photo — exactly so for shots of the same shape, which is what a
burst from one camera gives you, and approximately when you mix a portrait frame with a
landscape one, since each photo is fitted into its own pane. That is the point of comparing
two shots of the same moment at 200% on the same eye, rather than juggling the zoom on each
one separately. Each pane shows its file name, and — only where the panes actually differ —
its pixel dimensions and capture time; identical values on every pane would just be noise.

One pane is focused at a time. `1`–`4` or Tab (Shift+Tab to go back) moves the focus, `S` stars
the focused photo, Enter opens it in the viewer, and Escape (or the ✕ in the corner) closes
compare and takes you back to the grid exactly as you left it.

**photon does not delete.** Compare is for deciding which shot to keep; acting on that —
deleting the others — is your call, made outside photon with your file manager.

### Duplicates

photon finds two kinds of copy. Byte-identical files: after each scan it reads only the files
that share their exact size with another file, so on most libraries almost nothing is read.
And look-alikes: the same picture after a resize or a re-save, which is nothing like the same
bytes but is still the photo you already have. Look-alikes are found from the thumbnails
photon has already made for the grid, so no photo is read a second time to look for them.
A pair that looks alike at a glance is then compared picture to picture, so a second shot
of the same scene - the same person in almost the same pose, a burst - is not a look-alike:
it is a different photo, and you probably want to keep both.

While any exist, a **Duplicates** row in the sidebar shows every photo that has a copy of
either kind, folder by folder, and the viewer's info panel lists them under two headings,
Identical first, with pixel dimensions shown next to a look-alike (not next to an identical
copy, which is the same size by definition); clicking one locates it. Right-click a photo that
has copies and choose **Show N duplicates** to see just that photo and its copies in the grid,
or use the same link under the info panel's list. In the grid, a photo with a copy carries a
small copy mark in its bottom-left corner.
photon never deletes anything: use "Reveal in file manager" and decide there.

Settings → Duplicates → **Find look-alikes** has three settings: Off (byte-identical files
only), Conservative (the default: finds every close copy — resized, even to a small fraction
of its size, re-saved, or sent through a chat app), and Loose (also looks for more heavily
altered copies, at the cost of being best-effort there: it finds most of those, not all — a
real limit, not a bug). Either way a pair is only shown once the two pictures have been
compared, so a second shot of the same scene is not a look-alike. Changing the setting
regroups the library right away, without waiting for a scan.

On a fresh library, the first scan's look-alike pass usually finishes before the thumbnails it
needs have all been rendered, so newly indexed look-alikes tend to appear after the *next*
scan rather than the first one.

### Hiding photos

photon never deletes a photo, but it can put one away. Right-click a photo (or a selection)
and choose **Hide photo**, or press **H** - in the grid for the selection, in the viewer for
the photo on screen (press it again there to undo). A hidden
photo leaves every view, search, album, keyword and person, every count in the sidebar, and
Duplicates: hide the copy you don't want and its twin, no longer having a visible copy,
leaves Duplicates with it. The file is not touched.

While anything is hidden, a **Hidden** row in the sidebar shows those photos; right-click
there and choose **Unhide** to bring them back. The flag lives in photon's library, not in the
file, and follows the photo when it is renamed or moved inside the folders photon watches, so
it stays hidden.

To put a whole folder away, right-click it in the sidebar and choose **Hide folder**. Every
photo in it is hidden, and so is anything added to that folder later, until you choose
**Unhide folder** (from the folder's row under Hidden). Its subfolders are not affected; each
folder is hidden on its own, as in Picasa. Unhiding the folder brings back every photo in it,
including ones you had hidden one by one.

To call a folder something other than its directory's name, right-click it in the sidebar and
choose **Rename in photon…**. The sidebar and the grid show the new name; the directory on disk
keeps its own, which the grid still shows in the folder's path, and search finds the folder by
either. Empty the field, or choose **Use folder name**, to go back. The name lives in photon's
library and follows the folder when it is renamed or moved inside the folders photon watches,
so the folder normally keeps its photon name (and its **Hide folder** setting); "Renaming and
moving photos" below says when it does not.

Photos you hid in Picasa are hidden in photon too, and stay hidden when you rename or move
them. photon never writes Picasa's hidden flag, so a photo you unhide in photon stays visible
until you hide or unhide it in Picasa again.

### Slideshow and fullscreen

Press `S` in the viewer (or the ▶ button) to play the current view from the photo on screen:
fullscreen, crossfading, looping back to the start at the end. Space pauses, the arrow keys
and the wheel step, the controls hide while the pointer rests, and Escape ends the show and
leaves you in the viewer on the photo it stopped at. How long each photo stays is set under
Settings → Slideshow, and so is **Shuffle**, which plays the view in a mixed order: every
photo once before any comes round again, with photos that sit side by side in the grid kept
apart, so a burst is not three slides in a row. The arrow keys then step through that order,
so Left is the photo you just saw. A view of six photos or fewer than five plays in order.

`F11` toggles fullscreen at any time. photon remembers the window's fullscreen state, so if
you quit in the middle of a slideshow it reopens fullscreen; `F11` is the way out.

### Keyboard shortcuts

Press `?` to see every key photon answers, grouped by where it works - everywhere, in the
grid, in compare, in the viewer, while cropping, during a slideshow, on a video. It opens over
whatever you are doing, the viewer and compare included, and Escape or `?` again closes it;
in a text field `?` is just the character. The same list is under Settings → **Shortcuts**.
Where a shortcut uses Ctrl, a Mac shows ⌘.

A few worth knowing. `Ctrl+F` or `/` puts the caret in the search box, and Enter there moves
on to the photos it found. `.` stars what is selected - in the grid, in the viewer and in
compare - and takes the star off again; over a selection that is part starred, the photo the
selection is led by decides. In the viewer a double-click zooms to the photo's own pixels
where you clicked, and a second one fits it to the window again; `+`, `-` and `0` do the same
from the keyboard, and the wheel zooms where the pointer is while Ctrl is held. Without Ctrl
the wheel moves between photos, as it always has.

In the grid, Page Up and Page Down move the selection a screenful, in the same column. Hold
Shift with an arrow key, Home, End or a page key and the selection grows from where you
started to there, as a Shift+click does; go back the other way and it shrinks again. With
nothing selected the page keys simply scroll.

### Statistics

Settings → **Statistics** counts the library: how many photos and videos, how much disk they
take, the years they span, and how they spread over the years, your ten most used cameras and
your ten most used lenses. Every row is a link: click a year, a camera or a lens and Settings
closes onto a search for it. A year's search is exactly that year; a camera's or a lens's
matches the name word by word, so "EOS 5D" also finds an "EOS 5D Mark IV" and the grid can
hold more than the row counted. Hidden photos are not counted. The numbers are read when you
open the section, not kept live.

### Renaming and moving photos

Rename or move photos in your file manager and photon keeps what it knows about them. A photo
renamed, moved to another folder, or carried along with a renamed folder - anywhere inside the
folders photon watches, even from one watched folder to another - stays in its albums, keeps the
keywords you added in photon, its turns and crops, its hidden flag, and the faces photon found on
it with the names you gave them. A renamed folder normally keeps its name in photon and its
**Hide folder** setting; it does not when you made the new folder first and filled it
afterwards, because photon had already seen it as a folder of its own. It shows up under its new
name within a few seconds if photon is running, or on the next start if it was not.

photon does not compare pictures to recognise the photo. It reads the new file's header, as it
does for any new file, and a new file with the same size, the same modification time and the same
picture size and capture date as a photo whose old file is gone is that photo. If two photos fit
equally well, photon does not guess and treats the new file as a new photo; it does the same
among more than 32 files that are identical in size and modification time. Moving a file within
one drive is a rename and is followed. Moving between two drives is a copy followed by a delete:
a single photo is normally followed, but a long folder move between drives while photon is
running can be followed only in part, because photon may scan the copies while the originals are
still there, and then they are new photos.

What does not follow: a photo moved out of the watched folders and brought back later; a copy
whose original is deleted afterwards; a file you edited in another program and saved under a new
name; every photo of a watched folder moved into another one, leaving it empty (photon reads an
empty watched folder as an unplugged drive, and keeps its photos for when it comes back); a move
to a drive that keeps coarser timestamps, such as FAT or exFAT on a memory card (the
modification time changes); and the photos of a drive that is mounted inside a watched folder
and unplugged on its own (photon takes them for deleted). Picasa keeps its faces and albums in
the folder's `.picasa.ini` under each photo's file name, and a star is kept there too, whether
you set it in Picasa or in photon: all three follow a folder that is renamed or moved, not a
single photo that is renamed or moved on its own (see "Stars and Picasa").

photon does not look inside a drive's or a network share's recycle bin (`$RECYCLE.BIN`,
`#recycle`, `@Recycle`), so a photo you delete leaves the library, and its albums, even when the
bin lies inside a watched folder. Photos an earlier version indexed inside a bin leave the
library over the next two scans.

### Linux with an NVIDIA GPU

WebKitGTK, the webview photon uses on Linux, crashes on NVIDIA's proprietary driver when its
DMABUF renderer is on: the window flashes up and photon exits, printing `Error 71 (Protocol
error) dispatching to Wayland display`. photon detects the driver and sets
`WEBKIT_DISABLE_DMABUF_RENDERER=1` for itself, so nothing needs to be done. Rendering is a
little slower with the renderer off. A value you set yourself is always kept, so
`WEBKIT_DISABLE_DMABUF_RENDERER=0` turns the renderer back on if a later driver fixes this.

### Theme on Linux

photon follows the desktop's light or dark setting, but the system webview (WebKitGTK) does
not report it on every desktop. If photon stays light on a dark desktop, pin it in Settings →
Appearance.

### photon is not code-signed

Signing certificates cost money and are tied to a personal identity, so photon's installers
are unsigned. Every system says so in its own way, and none of it means the download is
broken:

- **macOS** refuses to open an app from an unidentified developer. Open **System Settings →
  Privacy & Security**, scroll down to the message naming photon, and choose **Open Anyway**,
  then confirm. (On macOS 14 and earlier you can instead right-click photon in Applications and
  choose **Open**.) You only need to do this once.
- **Windows** shows "Windows protected your PC". Choose **More info**, then **Run anyway**.
- **Linux** shows nothing; the AppImage just needs its executable bit.

Verify a download against the `SHA256SUMS` file attached to the release:
`sha256sum -c SHA256SUMS --ignore-missing`.

## Development

Prerequisites:
- Rust (stable, 1.93 or newer).
- Node.js 24 or newer (an LTS release; the UI toolchain — Vite 8, Vitest 5 and
  `@sveltejs/vite-plugin-svelte` — requires Node 22.12+, 24+ or 26+, so a plain "Node 22" install
  can be too old depending on its exact patch version. Node 24 is the current LTS line and is
  used in CI).
- The [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/) for your OS. On Linux that means webkit2gtk-4.1, libsoup-3 and librsvg2.

`mise.toml` pins the first two, so `mise install` gets you both at the versions CI builds
against. The Tauri prerequisites are system packages and still need your own package manager.

```bash
npm install        # installs the UI workspace and the Tauri CLI
npm run dev        # runs the app with hot reload
npm test           # UI unit tests
npm run check      # svelte-check
cargo test --workspace
cargo run -p xtask -- versions   # the three version files agree
cargo run -p xtask -- metadata   # licence and installer metadata are complete
cargo run -p xtask -- screenshots   # the built UI in headless Chromium, to target/screenshots/
```

Build a release bundle for this OS with explicit host-appropriate bundles, e.g. on Linux
`npm run tauri build -- --bundles deb,appimage` (the release workflow selects the right
targets per platform: `dmg` on macOS, `msi` on Windows).

Before each release, run the [manual smoke checklist](docs/smoke-checklist.md) on each OS.

## Adding folders

Settings → Folders → **Add folder…** opens the system's folder picker. Or drag one or more
folders from your file manager onto photon's window: photon says what dropping will do, and
watches each of them. A photo dropped on its own is refused with a note to drop its folder
instead - photon watches folders, it does not import files. Either way nothing is copied or
moved: the photos stay where they are.

## How watching works

photon watches each watched folder recursively for filesystem changes. When something
changes, it waits 2 seconds for things to settle, then rescans just the directories that
changed rather than the whole folder. If the OS won't grant a watch (for example the
system's watch-descriptor limit is exhausted), that folder falls back to a full rescan
every 5 minutes instead, and the status bar shows "Live updates limited" while any folder
is in that state.

## Licence

MIT. See [LICENSE](LICENSE).
