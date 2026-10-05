# Manual smoke checklist

Run this before each release, on each OS. The installed-app checks at the end can only be
done from a real installer, so run them against the draft release's artifacts before
publishing it.

- [ ] On first launch with a fresh profile, the Pictures folder is added and scanned without asking.
- [ ] Copy a few hundred photos into a watched folder while the grid shows it: as the scan adds them, the tiles already on screen never blink blank - they stay until the new rows replace them.
- [ ] Thumbnails appear within seconds, and scrolling stays smooth while indexing continues.
- [ ] **JPEG thumbnails through libjpeg-turbo.** Point photon at a copy of a folder of a few
  hundred real camera JPEGs (several cameras and a phone if you have them), with a fresh
  profile, on the previous release and then this one, with `RUST_LOG=photon_core=debug`.
  Note both imports' times until the last thumbnail is ready. On this release the thumbnails
  and the viewer's previews look as they did. Count the log's "libjpeg-turbo warned or failed"
  lines, each carrying the photo's path. More than a few percent of any one camera's files
  means a warning is sending them back after a nearly whole decode, and that goes back to
  the spec before release. Greyscale and CMYK files log "does not take this JPEG on" instead:
  that is a deliberate hand-back, expected, and not counted.
- [ ] **Edited photos through libjpeg-turbo's encoder, grid tiles through a box shrink.** Crop
  and turn a large camera JPEG, open it at 100% and pan around: no banding, blocking or colour
  fringes the previous release did not show, and it appears noticeably sooner. Export it with
  edits applied and open the copy in another viewer: the same picture, about the same file
  size as the previous release made. A fresh import's grid tiles look as sharp as the previous
  release's, with no moiré on fine patterns (fabric, fences, brickwork).
- [ ] **Previews at WebP method 1.** On a fresh import, open photos in the viewer before the
  full-size picture arrives (step quickly with the arrow keys): the preview looks as sharp and
  clean as the previous release's, with no new blocking in skies or smooth walls. The
  `preview` folder of the thumbnail cache is at most about a sixth larger for the same photos
  (a noisy photo's preview can come out smaller).
- [ ] In a large library, drag the grid's scrollbar (or the year strip) from one end to the other and let go: the photos on screen fill in within a moment of stopping. A slow wheel scroll shows each row's thumbnails as it arrives, with no wait, and End, Home and a sidebar folder click fill the screen at once. Open a photo, step through a few with the arrow keys, close it: the grid behind shows its thumbnails straight away.
- [ ] In a large library whose thumbnails are all cached, spin the mouse wheel as fast as it goes, then flick a trackpad hard, both down and back up: rows come onto the screen with their photos already in place, with no blank tiles filling in as they arrive. Only a scrollbar or year-strip drag, fast enough that whole screens go by at once, leaves the screen blank until it stops. End, Home and a folder click show cached photos at once, with no fade; on a fresh import, where thumbnails are still being made, each one still fades in as it arrives.
- [ ] **A library taller than the browser allows** (`cargo run -p xtask -- scroll-probe` checks
  the end is reachable; this checks how it feels). On a library of 150,000+ photos, or at
  large tiles in the narrowest window, and on Windows at 100%, 150% and 200% scaling:
  wheel and trackpad scroll at their usual speed; dragging the scrollbar thumb moves through
  the whole library and reaches both ends; after a pause the thumb jumps to where the grid
  is without the photos moving; End, Home, a folder jump and a timeline scrub land where
  they should; a rubber band dragged past the edge keeps selecting what it drew. On a
  normal library, nothing about scrolling has changed.
- [ ] In the viewer press `R`: within about half a second the photo is shown turned (preview first, then sharp), the caption's pixel size swaps, and back in the grid its tile is turned too. Press `R` four times quickly: it ends upright, not one turn short. Close and reopen photon: the turn is still there. The file on disk is unchanged (same size and date in the file manager).
- [ ] Press `C`: the whole photo shows with a bright rectangle and the rest dimmed; the eight handles resize, dragging inside moves, and nothing can be dragged off the photo. Choose 1:1: the rectangle becomes square *on screen* and stays square from every handle. Enter applies: the viewer shows only the crop, sharp at 100%, and the grid tile matches. `C` again shows the whole photo with the same rectangle. "Whole photo" then Enter removes the crop. Escape cancels without saving, and a second Escape is needed to close the viewer.
- [ ] Turn a cropped photo: the same part of the picture stays framed. With the info panel open on a photo with Picasa faces, the outlines still sit on the faces after a turn, and a face cropped out of the frame has no outline.
- [ ] "Original" appears in the bar only for an edited photo and restores it; its old thumbnail appears at once (it was still cached). A slideshow shows edited photos edited.
- [ ] Upgrade a real library from schema 8: no thumbnail is regenerated (the grid fills from cache as before).
- [ ] Copy a photo into another watched folder: within a scan the sidebar gains "⧉ Duplicates (2)", the view shows both files under their folders, and each one's info panel lists the other under "Identical"; clicking the path closes the viewer and lands on that copy. Edit or delete one of the two: after the next scan the row disappears.
- [ ] Save a copy of one photo at half its size into a watched folder, let the scan finish, and check both turn up under Duplicates with the copy marked "Looks the same" and its dimensions shown; clicking it locates it in the grid.
- [ ] Right-click a photo with copies: "Show N duplicates" appears with the right count, and does not appear for a photo without copies or for a multi-photo selection. Choose it: the grid holds the photo and its copies, the clicked photo is selected, and the sidebar shows "Copies of <name>" under Duplicates. Right-click one photo then quickly another: the item names the second one's count. Delete a copy on disk and wait for the rescan: the group shrinks, and with none left the grid says "No other copies of <name> any more."
- [ ] Open the Copies view on a photo that has a byte-identical copy and a look-alike, delete that photo and wait for two scans: the identical copy stays, the look-alike leaves, and the grid says the photo is no longer in the library and points to Duplicates.
- [ ] In All, a folder or a search, a photo with a copy shows a small copy mark bottom-left (a starred one shows both marks); a photo without one shows none, and in Duplicates and in a Copies view no tile is marked. Delete a photo's only copy and let the scan finish: its mark goes.
- [ ] In the viewer's info panel, a photo with copies shows "Show N duplicates in the grid" under the copies list, with the same N as the tile menu; clicking it closes the viewer and lands the grid on the Copies view with that photo selected, and the arrow keys work straight away.
- [ ] Open a camera photo's info panel: under Dates, "Taken, digitized" (or all three EXIF dates on one row) shows the capture time the caption shows, and "File modified" the time your file manager shows, in your own zone. A photo with no EXIF shows only the file's dates. On Linux, "File created" appears on ext4/btrfs and is simply absent on a filesystem or share that keeps no birth time.
- [ ] Open a photo that has both an identical copy and a look-alike: the info panel shows two headings, Identical first, and only the "Looks the same" entry shows dimensions.
- [ ] Settings → Duplicates → Find look-alikes: click through Off / Conservative / Loose. Each becomes pressed, the hint text below changes, and the Loose hint reads "Finds most look-alikes, not all of them." With it set to Off, Duplicates falls back to byte-identical files only; set it back to Conservative and the look-alikes return without a scan.
- [ ] Upgrade a real library indexed by an older photon: look-alikes appear without anything being re-scanned — the hashes come from thumbnails that already exist.
- [ ] Upgrade a real library from schema 7: the first scans finish as quickly as before, and duplicates appear shortly after the status bar stops showing a scan.
- [ ] In the viewer, `S` starts a slideshow: the window goes fullscreen, photos crossfade (no flash of black between them) at the interval from Settings → Slideshow, and the last photo is followed by the first. Space pauses and resumes; ←/→ step and the next photo then stays a full interval; the bar, zoom and ✕ fade out after the pointer rests about 2.5 s and return when it moves. Escape ends the show, leaves fullscreen and stays in the viewer; a second Escape closes the viewer.
- [ ] Press `?` in the grid: the shortcut sheet opens over it and lists the keys in seven groups, with ⌘ on a Mac where the others say Ctrl. Escape closes it and the arrow keys move the grid's selection again without a click. `?` in the search box types a question mark and opens nothing. Settings → Shortcuts shows the same list, and `?` does nothing while Settings is open.
- [ ] Press `Ctrl+F` (⌘F on a Mac) with the grid focused, with the sidebar focused, and on the People page: the caret is in the search box with its text selected, and no find bar of the webview's own appears (WebView2 has one). `/` does the same; typed *in* the search box or a name field it is the character.
- [ ] In the search box, type a query and press Enter: the grid has the focus and the arrow keys move the selection. Back in the box, Escape clears the search; Escape again on the empty box hands the focus to the grid. `Ctrl+F` with the viewer or Settings open does nothing.
- [ ] Select an unstarred photo in the grid and press `.`: it is starred; `.` again unstars it. Ctrl+click a starred and an unstarred photo, the starred one last: `.` unstars both; the other way round it stars both. `.` in the viewer and in compare flips the star on screen, and S in the viewer still starts the slideshow.
- [ ] Viewer: double-click a detail of a large photo. It zooms in with that detail still under the pointer, and once the full image has loaded it is sharp (one photo pixel per screen pixel, or 400% when the photo is larger than that). Double-click again: the whole photo, centred. Double-click a turn button: two turns, no zoom. A video does not zoom.
- [ ] Viewer: `+` (and `=`) zooms in, `-` out, `0` fits; the slider and the percentage follow. Hold Ctrl and turn the wheel: the photo zooms where the pointer is, and the page itself does not (WebView2, WebKitGTK). A trackpad pinch does the same where the webview reports one as Ctrl+wheel. The plain wheel still moves between photos.
- [ ] Viewer: drag the zoom slider, then without clicking anything else press `0`, `i` and `.`: the photo fits, the info panel opens and the star flips. The arrow keys still move the slider while it has the focus. On a video, click the volume slider and press Space: it pauses.
- [ ] Open a photo, press `?`: the sheet opens over the viewer. While it is up, the arrow keys, `H` and `R` do nothing to the photo behind it and Tab stays inside the sheet; Escape closes the sheet and not the viewer, and the arrows step photos again. The same over Compare (select two photos, `C`, then `?`): `1`-`4` and `S` are held, and work again after it closes. During a slideshow the sheet holds the show; closing it carries on.
- [ ] Start a rubber band in the grid and press `?` with the button still down: nothing opens.
- [ ] Start a slideshow from a window that is already fullscreen (F11 first): ending the show leaves it fullscreen. F11 toggles fullscreen from the grid, the viewer and Settings.
- [ ] Settings → Slideshow: the field shows the stored interval; entering 0 or 999 snaps to 1 or 60; the next slideshow uses the new value.
- [ ] Settings → Slideshow → tick **Shuffle**, then play a folder of a few dozen photos: the order is mixed, neighbours in the grid do not follow each other, no photo repeats before all have shown, and videos are still skipped. Left goes back to the photo just seen, Right forward again. Each change still crossfades (the next photo is not preloaded in a shuffled show, so on a slow disk the old photo holds a moment longer rather than cutting to black). Start a second show: the order differs. Untick it: the next show plays in order. The checkbox is still ticked after a restart.
- [ ] Tab into the grid with nothing selected: a ring shows around the photo area, so the keys' target is visible. Press ArrowDown and the ring gives way to a tile's selection ring. Clicking a tile shows no ring around the whole area.
- [ ] Shrink the window to its minimum width with the viewer open on a photo with a long file name: the toolbar's buttons stay clear of the zoom slider and the name ellipsises. Widen it again and the whole name comes back.
- [ ] Open the info panel over a bright, nearly white photo: the camera, lens and duplicate-path links and the "Add a keyword" placeholder are all comfortably readable against the glass.
- [ ] Double-click or Enter opens the viewer with an image in about 100 ms. The full resolution follows. ←/→, Home/End, Esc and Backspace work.
- [ ] In a folder of large photos, hold → for two seconds and let go: the previews flick past, and the photo it stops on turns sharp within about half a second rather than after a wait for every photo passed. A single → shows the next photo's preview at once and the sharp one a moment later.
- [ ] The mouse's back side button closes the viewer, and does not navigate the page or leave a blank frame. Worth checking on each OS: the three webviews deliver side buttons differently, and some consume them for history before the page sees them.
- [ ] The gear at the right of the top bar opens Settings. Escape, the close button and a click outside the dialog close it, and focus returns to the gear.
- [ ] Settings → Folders lists every watched folder with its path, photo count and status, and the status shows scan progress live.
- [ ] "Add folder…" in Settings adds a folder. Adding a folder inside a watched one is refused with a clear message.
- [ ] Rescan works from Settings and from a sidebar folder's right-click menu. "Remove…" in Settings asks first, works during a scan, and leaves the files on disk.
- [ ] Settings → About shows the version matching the release tag, and its "Reveal" opens the folder holding `library.db`.
- [ ] Settings → Statistics: "Counting…" gives way to a summary line (photos, videos, size, years) that matches the status bar's photo count in All photos plus the Videos count, then Years, Cameras and Lenses with bars. Click a year: Settings closes, the search box reads `from:2019 to:2019`, the grid shows that year and the arrow keys work in it without a click. Click a camera, then a lens: the same, with `camera:"…"` and `lens:"…"`, and the grid's count equals the row's number. Hide a photo and reopen Statistics: the total drops by one. On a large library the section opens without the dialog freezing.
- [ ] Settings → About shows Memory, updating every couple of seconds while About is open. On Linux and Windows it says "photon and its web view" and is close to the sum of photon's and its WebKit/`msedgewebview2` processes in the system monitor (PSS on Linux, Task Manager's Memory column on Windows); on macOS it says the web view is not included and matches photon's own Memory in Activity Monitor.
- [ ] With a few hundred tags, Settings → Tags stays centred in the window and the list scrolls inside the dialog.
- [ ] Settings → Tags lists every tag with its photo count, and the filter box narrows it. Rename a tag: Enter saves, Escape or clicking away cancels (Escape does not close Settings), a blank name is refused. The sidebar's Tags list and an open Tag view follow the new name.
- [ ] Renaming a tag to another existing tag asks to merge, then shows one tag whose count is the photos carrying either. "Remove…" asks first, and the tag disappears from the sidebar and from search.
- [ ] With the viewer's info panel open on a tagged photo, rename one of its tags in Settings: the panel shows the new name without reopening the photo, and the zoom and pan stay as they were.
- [ ] Rename a tag onto an existing one and decline the merge: the field stays open with what you typed.
- [ ] Each rename or removal appears under Changes, and "Restore" brings the original tag back. Rescanning the folder does not undo a rename, and the photo files' keywords are unchanged (check in another app).
- [ ] After removing every folder, the sidebar offers "Add a folder in Settings…", which opens Settings on Folders.
- [ ] Unplugging a drive with a watched folder dims its folder and tiles after a rescan. Nothing disappears.
- [ ] Type a search, then click the bookmark at the right of the box: a **Searches** group appears in the sidebar with the query as its name, and the bookmark fills. Click the bookmark again: nothing happens (it is inert, not a toggle). Clear the box and type the same query again — the bookmark is filled straight away, and with a trailing space too.
- [ ] Click the saved search in the sidebar from a different view: the box fills with the query and the grid shows its photos. Type fast in the box and click a saved search before the typing settles: the grid shows the saved search, not the half-typed text.
- [ ] Right-click a saved search → Rename…: the field opens with the name selected, Enter saves, Escape cancels. Delete… asks first; after deleting, the photos stay on screen and the box still holds the query. Both `lake OR pond` and `lake or pond` can be saved separately (capitals are operators, so they are different searches).
- [ ] Upgrade a real library from schema 10: it opens, and an older photon then refuses it with a clear message rather than a crash.
- [ ] Drop a `.tif` and a `.bmp` into a watched folder: both appear after the scan with correct thumbnails, and their tiles show the right shape (not stretched or letterboxed). Open each in the viewer at 100%. A TIFF the decoder cannot read (16-bit, CMYK, or JPEG-compressed — save one from GIMP or Photoshop to get one) shows the failed-thumbnail placeholder rather than an empty tile, and does not stall the folder's other thumbnails.
- [ ] Drop AVIFs into a watched folder: one from a phone (tiled, rotated), one exported by an editor, one with transparency. All appear after the scan, upright, with thumbnails whose shape matches the tile. Open each in the viewer at 100% on Windows, macOS and Linux. The full-size picture must be framed the same as its thumbnail. On a system whose web view cannot show AVIF (macOS 12 or older), the viewer stays on the sharp preview with no error. Turn one and crop one: the edit shows in the grid and the viewer. A truncated `.avif` (cut a copy in half) shows the failed-thumbnail placeholder and does not stall its folder.
- [ ] **rav1d's assembly (x86-64 installers only).** On an x86-64 machine, with a fresh profile, import a folder of a few dozen AVIFs on the previous release and on this one, and note both times until the last thumbnail is ready: this one should take well under half as long. The thumbnails look the same. The arm64 dmg is built without it and imports at the previous speed.
- [ ] Ctrl/Cmd+Shift+R and "Reveal in file manager" open the system file manager at the file.
- [ ] On Windows, add an SMB share (`\\server\photos` or by IP) as a watched folder: Settings → Folders shows it as `\\server\photos`, not `\\?\UNC\server\photos`, and "Reveal" there, on a sidebar folder and on a photo in it all open Explorer rather than failing. A library that already held the share from an older photon shows the same after the upgrade, with its photos, stars and albums intact (the share's thumbnails are rebuilt once).
- [ ] The viewer's caption reads name, capture time, resolution, size and position, e.g. `IMG_1234.JPG · Jun 15, 2024, 12:30 PM · 4000 × 3000 · 3.2 MB · (12 / 240)`. The capture time matches what the camera wrote, not shifted by your time zone.
- [ ] Clicking the caption copies the file name to the clipboard and shows "Copied" for about a second. Paste somewhere to confirm.
- [ ] Right-click a single photo in the grid, and right-click in the viewer: "Open in default app" opens the file in the system's image viewer (not in photon). It is missing from the grid's menu when several photos are selected. On a turned or cropped photo it opens the untouched file.
- [ ] Right-click in the viewer: "Locate in photon" closes it and lands the grid on that photo, selected and in view — also from Starred, Recent or a search, where it switches back to All first. "Reveal in file manager" opens the file's folder. Escape closes the menu before it closes the viewer.
- [ ] A library that cannot be opened (for example a corrupt database, or one written by a newer photon) shows an error dialog and photon exits, with no empty window sitting behind it.
- [ ] A watched folder whose drive is offline and which has never been scanned appears in Settings → Folders as Offline, and can still be rescanned, revealed or removed from there.
- [ ] A release build (see Development above) starts and shows the library — not a blank window.
- [ ] If a scan removes photos while the viewer is open, the viewer shows "This photo is no longer available" rather than a blank frame, and Escape still returns to the grid.
- [ ] Select a few photos, one of them turned or cropped, and right-click → "Export 3 photos…". Choose a folder: the path shows in the dialog. Export: the status bar counts up, a toast says how many landed and where, and the folder holds the copies — the edited one as you see it in photon, the others byte-identical to the originals (same size in the file manager). **The watched folder itself is unchanged**: same files, same sizes, same dates.
- [ ] Untick "Apply edits to the copies" and export the edited photo again: the copy is the original, uncropped picture, and the checkbox is still unticked the next time you open the dialog (it is remembered).
- [ ] Export a selection holding a large photo, a small one (under 1280 px), a video and a GIF with **Longest edge** at 1280 px: the large photo's copy is 1280 px on its long edge and looks right (not soft, colours as before), the other three are byte-identical to their originals. Open the dialog again: it reads **Original size**. The size list opens upwards and Escape closes the list without closing the dialog.
- [ ] Export into a folder that photon watches: as soon as you pick it the dialog says so in red, keeps itself open and refuses to export. A folder that merely *contains* a watched one (your home folder, with `~/Pictures` watched) is accepted.  Export a second time into a folder that already holds a copy of the same name: the new file arrives as `name (2).jpg` and the first one is untouched.
- [ ] Press on the grid and drag: a rectangle follows the pointer, every tile it touches rings as you go, and releasing keeps them selected — the status bar's count agrees. Dragging back over a tile takes it out again. Start the drag on a photo, not just on the space between them, and it still bands rather than selecting that one photo.
- [ ] Press and release without moving: that is still an ordinary click, and it selects the one photo. Drag a band that *ends* on a photo: the band's selection survives the release rather than collapsing to that photo.
- [ ] Hold Ctrl/Cmd (or Shift) while dragging: the band adds to what was already selected. Press Escape mid-drag: the rectangle goes and the selection you had before the drag comes back.
- [ ] Scroll the wheel mid-drag: the rectangle stays over the photos it was drawn across rather than sliding off them. Drag over a folder header: no text gets selected.
- [ ] Press Enter or double-click mid-drag: the viewer does not open and the grid does not keep scrolling behind it. (Touchscreen, if you have one: flick the grid to scroll it — the flick must not leave a rectangle behind, and afterwards a normal drag still starts a band.)
- [ ] **Hold the band near the bottom edge**: the grid scrolls under it and the band keeps growing, faster the closer to the edge you hold it, and the tiles it passes ring. Hold it near the top edge and it scrolls back. Let go: the count in the status bar matches what the rectangle covered, including rows that were still loading as they went past. At the end of the library it simply stops scrolling.
- [ ] **Drag a box over empty space** — below the last row, or beside a short one — and release: the selection is cleared, not put back. With Ctrl/Cmd held, the same drag leaves what was already selected alone.
- [ ] **Drag the grid's own scrollbar**: it scrolls, and no rectangle appears. (The scrollbar belongs to the same element the band listens on, so a press on the thumb arrives looking like a press on the grid.)
- [ ] Right-click a photo to open the menu, then drag a band elsewhere: the menu closes rather than sitting over the new selection describing the old one.
- [ ] Select several photos spanning two folders and right-click → "Add keyword to 12 photos…": the dialog opens with the field focused and lists the library's keywords; typing narrows them; Enter (or clicking one) writes it to all of them and a toast says how many took it. The keyword then shows in the sidebar's Tags list, finds all of them in search, and appears in each photo's info panel.
- [ ] "Remove keyword from …" on the same selection takes it off every one of them, including photos that carry it as their own metadata keyword — and it stays gone after that folder is rescanned.
- [ ] In the dialog, Escape and the × close it without writing anything, and the selection behind it is still selected. **Then press an arrow key straight away: the grid moves.** (Closing the dialog has to hand the keyboard back; the grid's keys live on the grid, so focus left on nothing makes them all dead.) While the dialog is open, Tab must stay inside it — the gear, the sidebar and the tiles behind the dimming are not reachable.
- [ ] Type a keyword you have renamed in Settings → Tags: the toast names the *new* name, and that is the name the photos carry. Remove a keyword from a selection twice: the second toast says it changed nothing rather than repeating the count.
- [ ] Open a photo, type a keyword in the info panel and press Enter: the chip appears at once, and the keyword shows in the sidebar's Tags list and finds the photo in search.
- [ ] Click the × on a keyword that came from the photo's own metadata: it goes from this photo only, and stays gone after the folder is rescanned.
- [ ] Rename that keyword in Settings → Tags: the photo's chip follows to the new name.
- [ ] With the viewer open on a photo, zoomed and panned, copying a photo into a folder that sorts *ahead* of it leaves the viewer on the same photo, at the same zoom and pan, with its caption renumbered. (The offset the viewer holds shifts when the grid is rebuilt; this is the check that it re-finds its photo rather than sliding onto the next one.)
- [ ] Start an import of a few hundred photos into a folder that sorts *ahead* of the one you are viewing, and arrow through large photos while it runs: each one still turns sharp after the preview, rather than staying on the blurry preview. A video playing meanwhile keeps playing, and a slideshow started during the import keeps advancing.
- [ ] Open a photo, arrow a few hundred photos on, star one (or resize the window), then press Escape: the grid lands on the photo you closed on, ringed, and Ctrl+C and H act on it straight away.
- [ ] Copying a photo into a watched folder makes it appear in the grid within a few seconds, with no manual rescan.
- [ ] Deleting a photo on disk removes it from the grid.
- [ ] Renaming a folder on disk moves its photos in the tree within a few seconds.
- [ ] Unplugging a watched drive dims it; plugging it back in restores it within about a minute, unattended.
- [ ] Typing part of a folder's name in the search box finds its photos.
- [ ] `from:2019 to:2019` shows only photos taken in 2019. Typing `from:2019-0` on the way to `from:2019-06` never blanks the grid or shows the wrong month in between: the half-typed term is ignored until it is a date.
- [ ] The search box stays put at the top of the window while the folder list and the grid scroll.
- [ ] The bar between the folder list and the grid resizes the list along its full height: dragging it stops at a minimum width and at half the window, the grid reflows to the new width, and with the bar focused (Tab) the arrow keys resize it too. The list's lower-right corner no longer has a resize grip.
- [ ] With a few thousand folders, dragging that bar stays smooth. Scroll the folder list end to end: nothing jumps or resizes as years come into view. Tab through the folders: the focus ring is whole on the first and last folder of every year, and on folders far below the part of the list already shown.
- [ ] Clearing the search box restores the full library.
- [ ] With photos from several years, a year strip shows right of the grid's scrollbar: years sit where they start, a line marks the current position and follows scrolling, hovering shows a year bubble, and pressing or dragging scrolls there — pressing on a printed year lands on that year's first folder, the same folder the sidebar lists first under it. The strip is absent in Recent, with a single year, and when everything fits on screen.
- [ ] `lake bell` shows only photos matching both words; `lake OR bell` shows photos matching either; typing `lake OR` on the way there keeps showing the `lake` results rather than flashing empty.
- [ ] Open the info panel on a dark photo, a bright one and a contrasty one: the histogram under the exposure row leans left, leans right, and has two humps. Crop a photo to its sky: once the new thumbnail is drawn the histogram moves right. A photo with a blown sky shows a full-height column at the right edge with the rest of the curve still readable. A video shows no histogram, and neither does a photo whose thumbnail failed.
- [ ] In the viewer's info panel, click the camera (then, on another photo, the lens): the viewer closes, the search box reads `camera:"…"`, and the grid shows that camera's photos, the clicked one among them.
- [ ] Search `is:starred`: the grid shows what Starred shows. `-is:starred` shows the rest. `tag:` with one of the sidebar's tags, `person:` with a Picasa name, `album:` with an album's name and `folder:` with a folder's name each show that tag's, person's, album's or folder's photos; a bare person's name finds nothing unless a file or folder is named so. Rotate a photo, then `is:edited` finds it. Typing `is:st` on the way to `is:starred` does not flash an empty grid when other words are in the box.
- [ ] Open a phone photo's info panel: **Location** shows coordinates that match where it was taken (check the hemisphere letters). "Photos nearby" closes the viewer, fills the box with `near:…` and shows that photo among the ones taken around it; adding `,25km` widens it. "Open in OpenStreetMap" opens the browser on that spot with a marker. A photo with no GPS (a scan, most cameras) shows no Location section. `has:gps` lists every photo with a position. After upgrading an existing library, positions appear once each folder has been rescanned.
- [ ] Click **On this day** in the sidebar: the box reads `on:MM-DD` for today's date, the row is highlighted, and the grid shows photos from that day in several years (or "no matches" if there are none - copy a photo's date into the query to check one that has). Add `to:2020`: only the earlier years stay, and the row is no longer highlighted. Clear the box: the library returns.
- [ ] Clicking a folder in the sidebar while a search is active leaves search and lands on that folder, and the search box empties. So does clicking Starred, Recent, an album, a person or a tag.
- [ ] Type a query, pause briefly, then keep typing without pausing again (e.g. type "beach", wait, then add "es" to make "beaches"): the box keeps every character you typed and never snaps back to an earlier, shorter query.
- [ ] Edit the search box, then immediately (within ~150ms) click a folder or Starred: the click's destination is what stays on screen — the grid must not jump back into a search a moment later.
- [ ] Click Starred and start typing a search straight away: the typed text stays in the box and the grid ends up showing that search, not Starred under an empty box.
- [ ] With Starred active and the search box empty, press Escape in the box: it stays on Starred rather than switching to All.
- [ ] With Starred or a search active, click a watched root (a bold top-level row) rather than a year row: it lands on that root's photos, the same as a year row does.
- [ ] Type text into the search box, then click a folder within about 150ms: the pending search is cancelled, so the box keeps the text (filtering nothing) until you clear it or press Escape.
- [ ] While a folder is being rescanned, the status bar shows "Scanning <folder>… N of ~M files (P%)" with a bar that fills, where M is the folder's photo count before the scan; a folder's first scan shows a moving indeterminate bar and a plain file count instead. Photos the scan adds are reported as "new or changed". The bar disappears when the scan ends.
- [ ] On a library large enough to exhaust the system's watch limit, the status bar says live updates are limited rather than silently missing changes.
- [ ] A folder added with "Add folder…" while photon is running picks up changes on disk within a few seconds, with no restart.
- [ ] Once a folder whose live updates were limited recovers, the status bar stops saying so without a restart.
- [ ] Select three photos in the grid and press `C`. Zoom into one corner with the wheel and drag: all three should move together, over the same part of each photo.
- [ ] Zoom past 100% on the focused pane and check it sharpens while the others stay as they were — only the focused pane asks for the full-size render.
- [ ] Start a pan and press Escape: the pan should stop and compare should stay open. Press Escape again to leave.
- [ ] Leave compare and check the grid's arrow keys, Enter and Escape still work without clicking first.
- [ ] Select one photo, then five, and check `C` does nothing and the menu offers no Compare; right-click 2–4 selected photos instead and choose Compare from the menu — it opens the same overlay.
- [ ] Select two photos with different pixel dimensions and press `C`: both panes print their size; select two with the same dimensions and it is left off both.
- [ ] Press `1`–`4` and Tab/Shift+Tab: the focus ring moves pane to pane, and Tab cannot walk out of compare into the grid or topbar behind it.
- [ ] Press `S` on the focused pane: a star badge appears on it, and the grid shows it starred after closing compare with Escape.
- [ ] Press `Enter` on a pane: the viewer opens on that exact photo and compare is gone.
- [ ] Drag to pan, release, then drag again: the second drag still works (a leaked pointer capture would kill it). Click the ✕ in the corner instead of Escape: it closes compare the same way.
- [ ] The downloaded installer runs, and photon starts from the installed location — not from a checkout.
- [ ] photon appears in the applications menu (Start menu, Launchpad) with the right name and icon.
- [ ] On Windows, the `.msi`'s install-folder page has an unticked "Create a desktop shortcut" box. Installing with it left unticked makes no desktop shortcut; installing again (over the top) with it ticked makes one; uninstalling removes it. Installing over 0.25.2 or older with it unticked removes the shortcut that version made.
- [ ] A freshly installed photon watches the Pictures folder on first launch, with no folder added by hand.
- [ ] On macOS, the permission prompt for the Pictures folder appears. Denying it shows "Live updates limited" in the status bar rather than silently indexing nothing.
- [ ] On Linux, `sudo apt install ./photon_*.deb` pulls in the webview dependencies on a clean machine, and photon starts rather than failing on a missing library.
- [ ] On Linux with NVIDIA's proprietary driver, under Wayland, photon starts and stays open, and logs "NVIDIA driver detected". Launched with `WEBKIT_DISABLE_DMABUF_RENDERER=0` it does not log that line (and, while the driver bug stands, crashes with `Error 71`).
- [ ] The security warning each OS shows matches what the README's "photon is not code-signed" section says to expect.
- [ ] `sha256sum -c SHA256SUMS --ignore-missing` passes against the downloaded files.
- [ ] On a freshly built library, photos rated in Picasa show under Starred with a matching count, once the first scan has finished.
- [ ] Clicking Starred shows only starred photos; clicking any folder returns to the full library at that folder.
- [ ] A library carried over from v0.2.0 picks up stars on the first scan of each folder, without being deleted and rebuilt.
- [ ] A folder starred in Picasa shows exactly those photos under Starred after a scan.
- [ ] Starring a photo in Picasa and rescanning makes it appear under Starred, without deleting the library.
- [ ] Un-starring a photo in Picasa and rescanning makes it disappear from Starred.
- [ ] After a full scan, no photo file's modification time has changed — photon never writes a photo file, and an INI changes only when a star is clicked.
- [ ] Starring a photo in the viewer writes `star=yes` into the folder's `.picasa.ini`; opening the folder in Picasa (or refreshing it) shows the star. Unstarring clears it there too.
- [ ] Starring a photo in a folder whose INI carries `faces=`, `filters=` or `backuphash=` lines leaves every one of those lines exactly as it was, and Picasa's face tags and edits for the folder survive.
- [ ] Starring a photo in a folder with only an old `Picasa.ini` edits that file and creates no `.picasa.ini` beside it; the folder's other stars stay.
- [ ] On Windows, `.picasa.ini` is still hidden after a star is toggled.
- [ ] On Windows, macOS and Linux: with a watched folder of many subfolders, star (or unstar)
  photos in 10 or more different folders at once from a selection: the stars show at once and
  the status bar shows no scan of the folder afterwards (an INI-only change is reread, not
  walked; a scan showing up here means the platform reports the folder itself as changed, and
  the fast path does not apply there).
- [ ] The tile of a starred photo shows a ★ badge, which appears and disappears with the toggle, and the sidebar's Starred count follows.
- [ ] Unstarring the photo on screen while Starred is showing keeps it on screen with no "n / m" in the caption; ArrowLeft goes to the previous starred photo, ArrowRight to the next, Escape returns to the grid.
- [ ] Select three photos in All, right-click, **Hide 3 photos**: they leave the grid, the photo after them is selected (ArrowRight carries on from there), and a **Hidden** row appears in the sidebar with a count of 3. Hidden shows exactly those three; **Unhide 3 photos** there brings them back, and the row disappears once nothing is hidden.
- [ ] In Duplicates, hide one of two byte-identical copies: both leave Duplicates and its count drops by two. The same with two look-alikes.
- [ ] In the viewer, right-click → **Hide photo**: the photo stays on screen without "n / m", ArrowRight goes on to the next photo, and the right-click menu now says **Unhide photo (H)**. Escape: the grid no longer holds it.
- [ ] In Duplicates, select a photo and press **H** a few times: each press hides the selected photo and selects the next one, and the grid scrolls to keep it in view. In Hidden, **H** unhides. With Ctrl, Alt or Cmd held, **H** does nothing.
- [ ] In the viewer, **H** hides the photo on screen (it stays showing, the menu now says **Unhide photo (H)**) and a second **H** unhides it. With the zoom slider focused, **H** does nothing.
- [ ] Right-click a folder in the sidebar → **Hide folder**: it leaves the sidebar and its photos leave All; in Hidden the folder is listed and its right-click menu says **Unhide folder**. Copy a new photo into that folder: after the scan it is in Hidden, not All. A subfolder of it stays visible. **Unhide folder** brings every photo back.
- [ ] Right-click a folder → **Rename in photon…**: the field opens pre-filled and selected; type a name and press Enter. The sidebar and the grid header show it, the header's path still ends in the directory's name, and the directory on disk is unchanged. Sort by name: the folder sorts by its new name. Search for it: its photos are found. Rename it to an empty field: the directory name returns. Set it again, then **Use folder name** from the menu does the same. Escape in the field changes nothing.
- [ ] Open a photo from Hidden, right-click → **Locate in photon**: the grid lands on it in Hidden, not in All. In Hidden, clicking a folder in the sidebar stays in Hidden.
- [ ] Open the Copies view of a photo with one identical copy and hide the photo from its tile menu: the copy stays, with the line "… is hidden; these are its copies". In Hidden, that photo's menu offers no "Show 1 duplicate".
- [ ] Open a photo in the viewer from a selection, **Hide photo**, Escape: nothing is selected, and Ctrl+clicking another photo says "1 selected", not 2.
- [ ] Add a folder whose `.picasa.ini` has `hidden=yes` for one photo: that photo is in Hidden, not in All. Unhide it in photon and rescan: it stays visible.
- [ ] Hide a starred photo that has a keyword and is in an album: Starred's, the keyword's and the album's counts in the sidebar each drop by one, and a search for its file name does not find it.
- [ ] Clicking ★ while zoomed in toggles the star and does not start a pan.
- [ ] In the viewer, `R` turns the photo clockwise and `Shift+R` anticlockwise, as do the ↻ and ↺ buttons; a landscape photo turned on its side fits the window's height rather than being clipped, zoom and pan still work on the turned photo, and the next photo opens upright. No file's modification time changes.
- [ ] The ⓘ button (or `I`) opens the info panel with the camera, lens and exposure line for a photo from a camera, and "No camera data" for a screenshot. Esc still closes the viewer with the panel open.
- [ ] A photo with keywords written by Picasa, Lightroom or digiKam lists them under Keywords in the info panel, and each appears under Tags in the sidebar with a count; clicking a tag shows exactly those photos.
- [ ] A folder whose `.picasa.ini` names faces lists the names under People in the info panel, outlines each face over the photo while the panel is open (also after `R`), and lists the person in the sidebar's People group; clicking the person shows their photos. Naming a new face in Picasa appears within a rescan, without the photo changing.
- [ ] "New album…" in the sidebar takes a name on Enter and cancels on Escape or an empty name. A tile's right-click menu adds the photo to an album; in the album view it offers "Remove from". The info panel's checkboxes add and remove too, and the sidebar count follows. Rename and Delete… work from the album's right-click menu, and deleting the album on screen returns to All.
- [ ] On a real Picasa library, `grep -rl --include='*icasa.ini' '^\[\.album:' <library>` finds INIs, and their albums appear under Albums with the Picasa icon and Picasa's photo counts. **This confirms the INI album format, which was designed from documentation; if it fails, the parser changes before release.**
- [ ] Right-clicking a Picasa album opens no menu; the grid's "Add to album" does not list it; its view offers no "Remove from".
- [ ] The info panel lists the photo's Picasa albums below the album checkboxes, without checkboxes.
- [ ] Renaming an album in Picasa and rescanning renames it in photon.
- [ ] Rename an album in Picasa (which rewrites every member folder's INI) with photon
  running: the album's new name shows after a moment, with no full scan.
- [ ] A photo captioned in Picasa shows its caption under the photo, at the top of the info panel, and during a slideshow after the controls fade.
- [ ] Searching a word of that caption finds the photo.
- [ ] A photo whose camera wrote only an EXIF description (e.g. "OLYMPUS DIGITAL CAMERA") shows no caption.
- [ ] Typing a camera name, a lens, a keyword, `50mm`, `f/1.8`, `iso400` or a date like `2024-06` in the search box finds the matching photos.
- [ ] On a library built by v0.12 or earlier, the first scan after upgrading fills in camera data and keywords for existing photos without changing their thumbnails, and the second scan does not re-read them.
- [ ] Clicking Recent shows the newest photos first across folders, capped at 500, as one continuous run of tiles with no folder headers and no gaps where the folder changes.
- [ ] While Recent is shown, the sidebar lists each contributing folder once with the photos it contributes, and the viewer's caption counts through the 500 rather than restarting at "1 / 1" on every photo.
- [ ] From Recent, clicking a folder or a watched root returns to the full library at that folder.
- [ ] Scrolling to a folder partway down the library, quitting and reopening opens the grid at that folder — and closing from Starred or Recent instead still returns to the folder that was last browsed in the library view.
- [ ] Removing the watched folder the grid was last showing, then reopening, lands at the top of the library rather than failing.
- [ ] Maximising the window, quitting and reopening brings photon back maximised (the case this was built for, on Windows).
- [ ] Resizing and moving the window, then quitting and reopening, restores that size and position — including on a second monitor, while it is still attached.
- [ ] Unplugging the monitor a window was last on and reopening puts photon on a screen that exists, rather than off-screen.
- [ ] A library that cannot be opened still shows the error dialog, and the launch after it has a visible window — window state must never carry the hidden window forward.
- [ ] Settings → Appearance → Dark, then Light, then System: the whole window follows at
      once, and so does the title bar. With System, switching the desktop between light
      and dark switches photon without a restart.
- [ ] On a light desktop: Settings → Appearance → Dark, then System: photon returns to light
      without a restart.
- [ ] Pin Light on a dark desktop (or Dark on a light one), quit, relaunch: the first frame
      is already the pinned theme, with no flash of the other - and so is the title bar, from
      the moment the window appears, not a second later.
- [ ] Pin a theme, then change the desktop's scheme: photon does not move.
- [ ] Checkboxes, the crop ratio menu and the scrollbars match the theme.
- [ ] Tab through the top bar, sidebar and Settings: every control shows the same blue
      focus ring, and opening the viewer or Settings draws no ring around the window.
- [ ] In Light and in Dark: the top bar, sidebar and status bar are one tinted surface with
      hairline dividers; the active sidebar row is a blue-tinted pill and its count stays
      readable; hovering a row tints it.
- [ ] Sidebar icons (star, clock, copies, the chevrons and the three group icons) are crisp
      and follow the text colour; no emoji or box glyph appears anywhere in the shell.
- [ ] Tab through the sidebar: the focus ring shows whole on every row, not clipped at the
      panel's edge. Drag the splitter, and resize it with the arrow keys, as before.
- [ ] A long album or folder name ellipsises and its count stays right-aligned at every
      sidebar width.
- [ ] In Light and in Dark: a selected tile has a blue ring drawn inside its edge, whole on
      all four sides, that stays visible over a white, a black and a blue photo; selecting
      does not resize the photo; arrow-key selection shows the same ring, whole and
      unclipped, including after ArrowUp/Home and on Recent's first row.
- [ ] Scroll a long library top to bottom, then drag the timeline: folder headers never
      overlap the first tile row, and the year bubble follows the pointer.
- [ ] A starred tile's amber star is legible over a bright photo; a photo that cannot be
      shown has a warning icon, not an emoji.
- [ ] Right-click a tile in Light: the menu is distinct from the grid behind it. Provoke an
      error toast (open an offline folder's photo): it is readable in both themes.
- [ ] With photon in Light, open a photo: the viewer is black and its toolbar, info panel,
      zoom control, menu and checkboxes are all dark, with the dark theme's blue.
- [ ] The toolbar reads over a white photo and over a black one; on a machine where the
      blur is missing it is still legible. Star, rotate, crop, slideshow and info all still
      work, and a disabled tool looks disabled.
- [ ] Crop: the rectangle, its eight handles and the dimming outside it are where they were;
      dragging a handle, Apply, Cancel and Whole photo behave as before.
- [ ] Info panel open, star a photo, add and remove a keyword, tick an album: the photo on
      screen does not reload, and zoom and pan are kept.
- [ ] Slideshow: after a few seconds without the pointer the toolbar, zoom and close fade
      out, and come back when it moves.
- [ ] At the minimum window width (800px), open a photo with a long file name: the toolbar
      and the zoom control may meet; nothing becomes unreachable.
- [ ] On Windows, with photon in Light: open the crop tool and its ratio list — the list is
      readable (dark text on light, or light on dark, never light on light).
- [ ] Settings in Light and in Dark: the header and section list are tinted, the content is
      not; the active section is a blue-tinted row; every button changes shade slightly under
      the pointer, "Add folder…" is the one blue button, and "Remove…" is red text.
- [ ] Settings → Tags with hundreds of tags: the dialog stays centred and its corners stay
      rounded while the list scrolls.
- [ ] Ctrl+click (Cmd on macOS) three photos: each gets a ring, the status bar reads
      `3 selected`. Shift+click a fourth further down: the run between the last Ctrl+click and
      it is selected. A plain click anywhere collapses back to one.
- [ ] Right-click inside a selection: the menu reads `Star 4 photos`, and Reveal is absent.
      Right-click a photo outside it: the selection collapses to that one first.
- [ ] Star a selection spanning two folders, then check both `.picasa.ini` files: each holds a
      `star=yes` under every selected photo's header, and every other line it had is untouched.
- [ ] Select several, open one with Enter and close it again: the selection is still there.
      Arrow to another photo inside the viewer and close: only that photo is selected.
- [ ] In the library view, click a photo and press Ctrl+A (Cmd+A on macOS): its folder is
      selected and nothing above or below that folder's header is. In Starred, an album or a
      set of search results, Ctrl+A takes the whole view. Escape clears the selection — with
      the context menu open, the first Escape only closes the menu.
- [ ] Click the sidebar, the status bar or a folder name and press Ctrl+A: **nothing**
      highlights. No blue wash over the chrome, no selected folder names. Then click into the
      search box, type a word and press Ctrl+A: the query is selected, as in any text field.
- [ ] Scroll the grid past the middle of the library and make the tiles **smaller** in the top
      bar (Large → Small is the strongest case): the photo that was at the top of the screen is
      still at the top afterwards — not a different year, and not the end of the library. Then
      make them bigger again and check the same. Try both with a folder header the first thing
      visible: the same header is still at the top.
- [ ] Change the size in Recent (no headers) and in a Starred or search view too: your place is
      kept there as well. Try it scrolled to the very end of the library: it lands at the end,
      not past it.
- [ ] Change the size from Settings → Appearance instead of the top bar: the top bar's control
      moves to match, and changing it back in the top bar moves Settings' control too.
- [ ] Tab to the size control, in the top bar and in Settings → Appearance: each size button is
      its own Tab stop, pressing it shows a focus ring, and Enter or Space activates it.
- [ ] Open the viewer and press Escape; the arrow keys must still move the grid. Then open
      Settings and close it, and check the same. (The focus-ring rule was scoped in this work,
      so it is worth re-checking that Settings, the context menus and the viewer show no ring
      around their own outer surface, and that a selected tile still shows only its blue inset
      ring, never an outline-style focus ring, at every size.)
- [ ] At Small, drag a rubber band across two rows and check it selects what it covers,
      including edge autoscroll. Repeat at Large. Arrow-key and Home/End navigation keep the
      selection on screen at both sizes too.
- [ ] Quit with a non-default size chosen and relaunch: the grid opens at the remembered
      folder, at the size you left it, with no visible jump to the top first.
- [ ] In the viewer, Ctrl+C (⌘C on macOS) on a turned and cropped photo, then paste into a
      chat app or document: the pasted picture is turned and cropped, and says "Photo copied".
      Repeat from the grid (one photo selected, Ctrl+C and the menu's Copy photo); with two
      selected, the menu has no Copy photo. Triple-click a folder header, click a photo, press
      Ctrl+C: the photo is copied, not the header text. Hold Ctrl+C down: one "Photo copied".
- [ ] Select a caption's text in the info panel and press Ctrl+C: the text is copied, not the
      photo.
- [ ] Scroll the full library to some folder, open Starred, then click All photos: the grid is
      back at that folder, and All photos is highlighted. From a search, the search box empties.
- [ ] Put an iPhone `.MOV`, an Android `.mp4` and a `.webm` in a watched folder: each gets a tile
      with a play badge and its length, sorted among the photos taken beside it (not hours away).
      The preview frame is not black.
- [ ] Open each: it plays at once with sound; the seek bar works, including a jump to the last
      tenth of a long 4K video; Space pauses and resumes; ←/→ go to the neighbours and the video
      stops; R and C do nothing; the zoom slider is gone. The info panel shows the length.
- [ ] A slideshow started in a folder of photos and videos shows only the photos; started on a
      video it begins at the next photo; in a folder of only videos it does not start and says
      why.
- [ ] Linux, without GStreamer's good plugins (remove them, or run on a fresh Arch): photon
      starts, video tiles show the play icon, opening one says the plugins are missing, and the
      window never goes blank.
- [ ] The AppImage, on a machine with GStreamer's good and libav plugins installed, plays a
      video; on one without them it shows the can't-play message and never goes blank.
- [ ] macOS and Windows: a video plays (App Transport Security and WebView2 allow
      `http://127.0.0.1`), and no firewall prompt appears when photon starts. **Blocking for the
      release.**
- [ ] Windows without the HEVC extension: an iPhone video shows the play icon and a message, not
      a broken tile.
- [ ] A video's poster frame actually appears (this exercises the raw-body `put_video_frame` IPC
      path and CORS on the loopback server). If frames never appear, check whether WebKit sends a
      CORS preflight (`OPTIONS`) for a `crossorigin` `<video>` with a `Range` header: the media
      server answers `OPTIONS` with 404.
- [ ] photon's video controls sit above the viewer's bar and look the same on every OS. After
      clicking any of them (play, mute, the volume slider, loop), the arrows, Home/End and
      Escape still navigate the viewer and Space toggles playback once, not twice. Clicking the
      picture plays and pauses.
- [ ] Drag along the position bar: the picture follows the pointer, and letting go resumes
      playback if it was playing. Press Escape mid-drag: the video jumps back to where the drag
      began. Shift+←/→ moves 5 seconds.
- [ ] Loop (the button or L): the video starts over at its end instead of stopping. Open another
      video: loop, mute (M) and the volume are still as you left them; restart photon and they
      are back to loop off, sound on.
- [ ] The picture is never covered by the controls or the bar, on a landscape and a portrait
      video.
- [ ] The mouse wheel over a playing video still moves to the next and previous item.
- [ ] The sidebar has a **Videos** row with the right count once the library has a video, and
      none in a library of photos alone. It shows every video under its folder, sorted by each
      folder's oldest video; hiding a video takes it out of the row's count and the view.
- [ ] In the grid, right-clicking a video offers no "Copy photo", and Ctrl+C on a selected video
      does nothing.
- [ ] Pause a long video, open three more, then play one: playback starts at once.
- [ ] Quit photon while poster frames are still being drawn, and relaunch: the frames carry on
      being drawn, and no video shows "photon's window stopped while opening this video".
- [ ] Tile badge placement: the play badge is top-left, because the star and the copies mark own
      the bottom corners. Check that it stays legible on light and dark photos.
- [ ] Sort (the menu beside the photo sizes): Name, Size and Date modified each show one grid
      with no folder headers and no year strip, and the sidebar lists folders without year
      headings, by name, by total size or by newest change. Clicking a folder there scrolls to
      its first photo. The reverse button turns both over; Date taken reversed keeps the folder
      headers, oldest folder first. The menu, open and closed, looks the same on
      every OS and in both themes: photon draws it, like the right-click menu.
- [ ] photon's dropdowns (the sort menu, the crop tool's ratio menu, which opens upwards):
      click opens and chooses; a click elsewhere closes. From the keyboard: Tab to it, arrows
      or Enter open it, arrows move, typing a letter jumps, Enter or Space chooses, Escape
      closes without choosing. In the crop tool, Enter or Escape used on the open ratio list
      neither applies nor cancels the crop, and with the list closed they still do. A screen
      reader announces the option being moved to.
- [ ] In All sorted by Size, select a photo and press Ctrl+A: only that photo's folder is
      selected, wherever its photos sit in the list.
- [ ] Choose a sort, quit and relaunch: photon opens in the same sort. Back on Date taken, the
      grid opens at the folder last browsed, as before.
- [ ] Changing the sort with text in the search box keeps the search, now in the new order.
- [ ] Drag a folder from the file manager over photon's window: a card says "Drop folders to add them to photon"; drag out again and it goes. Drop it: a toast says "Watching “name”", the folder appears in the sidebar and under Settings → Folders, and its scan starts. Drop two folders at once: "Watching 2 folders". Drop a single photo: one toast says it is a file, not a folder, and nothing is added. Drop a folder that is already watched, or one inside a watched folder: no duplicate appears (the second says it overlaps). Do this once with the viewer open and once with Settings open. Check on each OS: the drag is the system's, and each webview reports it differently.

## Face detection

- [ ] Settings → People: the switch is off on a library that never had it on.
- [ ] Switch it on: the status bar shows "Finding faces: N of M" at once, before the first photos are done, and N rises; the same line is under the switch in Settings.
- [ ] Quit mid-pass and relaunch: it carries on from where it was, not from zero. Do it once with the photos' drive unplugged: it still carries on.
- [ ] Open a group photo with the info panel shown: every face has an outline; Picasa's named faces keep their name plates and are not outlined twice. The People list ends with "N faces not named".
- [ ] Open a head shot or a selfie, where one face fills most of the frame: it is outlined, and the outline is around the whole face.
- [ ] Search `-has:tag`: only photos with no keyword. `has:caption` shows the captioned ones. `is:duplicate` shows what the Duplicates row shows. `is:portrait` shows upright photos only, including one you turned in photon. `size:>10mb`, `iso:>=1600`, `aperture:<2` and `mp:<2` each show photos whose info panel agrees; typing `size:>10` before the `mb` does not flash the whole library.
- [ ] `has:face` finds photos with people; `faces:2` and `faces:3+` find the right ones; `-has:face` finds landscapes.
- [ ] Turn or crop a photo with detected faces: after its thumbnail is remade, the outlines are on the faces again.
- [ ] A detection landing on the photo open in the viewer (open one the pass has not reached, zoom in, wait): the photo does not blank and the zoom stays.
- [ ] A photo stored sideways with no orientation tag (a scan, or a file whose EXIF was stripped): its faces are mostly not found. Turn it upright in photon: once its thumbnail is remade it is looked at again and the faces are outlined.
- [ ] Switch it off mid-pass: the progress line clears and stays cleared, the outlines of detected faces are gone, Picasa's remain.
- [ ] Toggle the switch on and off quickly several times, ending on off: no progress line comes back, and after a relaunch the switch is still off.
- [ ] A Picasa library with unnamed faces, detection off: those faces are outlined without a name.
- [ ] The app stays usable (scrolling, opening photos) while a pass runs on a large library.
- [ ] Unplug a drive whose photos already have thumbnails, with the pass unfinished: the pass carries on through them.
- [ ] Windows and macOS: the same switch-on, on a real library.

## People

On a real library, with "Find faces in my photos" on. The first block is the recognition pass
itself; the second is the People page.

- [ ] A library a 0.47.0 photon already detected, opened with the switch on: the status bar shows "Recognising people: N of M faces" at once, without "Finding faces" first going over every photo again, and N rises; the same line is under the switch in Settings. It clears when the pass ends.
- [ ] On a library detected from scratch: "Finding faces" runs first, then the line changes to "Recognising people", in faces rather than photos.
- [ ] Quit while recognising and relaunch: it carries on from where it was, not from zero.
- [ ] While it runs and after it: the sidebar's People list still shows Picasa's people with their counts, clicking one still shows their photos, and `person:` with a Picasa name still finds them.
- [ ] The viewer's info panel on a photo with Picasa faces: the name plates are still there, each face drawn once, and photon's own detections are still outlined without a name.
- [ ] The app stays usable (scrolling, opening photos, starring) while a large library is being recognised.
- [ ] Open the library with photon 0.47.0 afterwards: it refuses it as made by a newer photon.
- [ ] Switch on, wait for "Recognising people", then click People in the sidebar: the page opens over the grid, with Unnamed groups largest first, each face a crop of the face.
- [ ] The sidebar's People row says "N to name"; naming a group lowers N, and the count goes back to the number of people when none is left.
- [ ] Name a group: type a name, Enter. The group moves to People and the sidebar's list gains the person.
- [ ] Type a name that already exists in another case ("anna" for "Anna"): the hint by the field says "Add to Anna · Enter" (it is not a button), and Enter merges the group in.
- [ ] Select a face and choose "Not this person": it leaves the group and is placed again, never into that group.
- [ ] "Ignore" on a group moves it under Ignored; "Stop ignoring" brings it back to Unnamed.
- [ ] Suggestions: faces photon thinks are someone you named, dashed. "Confirm all" confirms the faces on screen and its label says how many when that is fewer than the person's suggestions.
- [ ] A large group: "Show more (N left)" loads up to 200 more at a time; after three clicks, select a face and act on it, and the strip stays open with every face it had.
- [ ] With "Recognising people" running for over a minute, type in a group's name box and act on faces: the field keeps focus and the rows do not move (new groups appear at the end of Unnamed).
- [ ] A library with more than 200 unnamed groups: Unnamed lists 200, its heading counts them all, and a line below says "Showing the 200 largest groups. Name or ignore some to see the rest."
- [ ] While the People page is up, Sort and Size are hidden and the search box and the gear stay where they were; they come back with the grid.
- [ ] On a library with no previews yet, open the People page with Find faces off, switch it on in Settings and close Settings: the page no longer says detection is off.
- [ ] Double-click a face: the viewer opens over the page; closing it returns to the page with its state; the arrows browse the grid's view (All photos when the view lacked the photo).
- [ ] Rename a person to a name another person has: the two merge. "Merge into…" and Delete (the group returns to Unnamed) ask first with the native dialog.
- [ ] Opening a person from the sidebar's list, and `person:` in search, show only confirmed faces.
- [ ] Leaving the page by any sidebar row, a folder, or typing in the search box returns to the grid at the place it was left.
- [ ] A face whose photo is hidden is nowhere on the page, and not in the count.
- [ ] Switch detection off with named people: the question names the count; Cancel keeps everything and the switch stays on; OK deletes the detections and people, and the People page says detection is off.
- [ ] Switch it off while recognising: the progress line clears and stays cleared; Picasa's people stay in the sidebar and the viewer.

## People from the grid and the viewer

On a real library with "Find faces in my photos" on, recognition finished, and at least one
named person (Anna below) and a Picasa contact linked to her by name.

- [ ] Grid, one photo with one unnamed face: right-click, "Add photo to a person…", choose Anna. The toast says "Added 1 photo to Anna."; the photo is in Anna's view.
- [ ] Grid, a selection mixing a photo with several unnamed faces, one that is already Anna's, and one with no face: the toast lists the several-face photos by file name ("open them to choose the face"), says one is already Anna's and one has no unnamed face, and adds only the one-face photos.
- [ ] Type a new name in the dialog: the hint says "New person “Ben”"; Enter makes Ben, and he appears in the sidebar's People list.
- [ ] In Anna's view, select photos and "Remove N photos from “Anna”": they leave the view and the toast says so. A photo where Picasa names Anna stays, and the toast says "Picasa names Anna on it".
- [ ] In a Picasa contact's own view (`c:`, no person linked), the tile menu has no Remove item.
- [ ] Viewer, right-click an unnamed face (info panel open or closed): "Name this face…" opens "Who is this?"; naming it puts the plate on the face without the photo blinking or the zoom resetting.
- [ ] Viewer, right-click Anna's plate: "Not Anna" takes her off; the toast says "Removed 1 photo from Anna." In Anna's view the viewer stays on the photo, and the arrows carry on from where it was.
- [ ] Viewer, "Not Anna" on a photo where Picasa also names Anna (a contact linked to her, another face): the toast says "1 stays with Anna: Picasa names Anna on it." and the photo stays in her view.
- [ ] Viewer, right-click away from any face on a photo with Anna and Ben: the menu offers "Not Anna" and "Not Ben" above Locate. A face only Picasa knows (no detection beneath) offers nothing, and a person only Picasa names on the photo is not offered.
- [ ] Viewer, zoomed in and panned: right-click a face: the menu is for that face, not one beside it.
- [ ] With the info panel open, click an unnamed outline: the dialog opens for that face. Zoomed in, clicking an outline does not start a pan. Tab reaches the outlines and their focus ring shows.
- [ ] With the dialog open over the viewer, type "h", "r", "s" and the arrows in the field: nothing happens behind it. Click the dialog's title (off the field) and press "h", then Escape: the photo is not hidden, and Escape closes only the dialog, not the viewer. Tab never reaches the viewer's controls (the viewer is `inert` in a `display: contents` wrapper - unverified in WebKitGTK and WKWebView).
- [ ] Closing the dialog over the viewer leaves the viewer's keys working at once (arrows, Escape).
- [ ] During a slideshow, right-click an unnamed face and "Name this face…": the show does not move on while the dialog is open, and after it closes the photo stays a whole interval before the next.
- [ ] Hide every photo of a named person (Ben), then open the dialog and type "ben": the list offers Ben and the hint says "Add to Ben", not "New person".
- [ ] Switch "Find faces in my photos" off in Settings: the grid's tile menu no longer offers "Add … to a person…"; switch it on again and it does, without restarting.
- [ ] Click Anna's name in the info panel's People list: the viewer closes and Anna's view opens, the grid focused (the arrow keys move the selection). A name on a photo twice (Picasa and photon) is listed once.

## Renaming and moving photos

- [ ] Put a photo in an album, turn it, add a keyword. With photon running, rename the file in the file manager: within a few seconds the grid shows the new name, and the photo is still in the album, still turned, still tagged. Its thumbnail does not flash to a placeholder.
- [ ] Quit photon, rename a folder that has a photon name, a hidden photo and a named face in it, start photon: the folder keeps its name, the photo is still hidden, the person's view still lists their photo.
- [ ] Move a photo from one watched folder to another: it keeps its album. If the two folders are on different drives, a move is a copy then a delete: a single photo is normally still followed, but a long move of a whole folder between drives while photon is running may be followed only in part, the rest arriving as new photos with no album. Moving within one drive is followed.
- [ ] Copy a photo (keep the original): the copy is a new photo with no album; the original keeps its own.
- [ ] Unplug a drive photon watches, copy some of its photos from a backup into another watched folder: the copies appear as new photos; plug the drive back in and its photos still have their albums.
- [ ] Move a photo out of every watched folder, let photon scan twice, then move it back: it is a new photo with no album (this is not followed).
- [ ] Hide a photo in Picasa (or add `hidden=yes` under its `[name]` in the folder's `.picasa.ini`) and let photon scan: it is in Hidden. Rename the file in the file manager: it is still in Hidden, under its new name, and not back in the grid. Unhide another photo Picasa hid in photon (its `hidden=yes` line stays), then rename its folder: that one is still visible.
- [ ] On Windows with a drive's root watched (or on a NAS share with its recycle bin switched on), put a photo in an album and delete it in Explorer: within a few seconds it leaves the grid and the album, and it does not turn up again in a folder named `$RECYCLE.BIN`, `#recycle` or `@Recycle`.
- [ ] Star a photo, then rename the file in the file manager: the star is gone (it is kept in `.picasa.ini` under the old name). Star another photo and rename its *folder* instead: the star stays.
