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
| Windows | `.msi` | Run it. |

### Upgrading to a version with Starred photos

photon reads star ratings from Picasa's per-directory `.picasa.ini` / `Picasa.ini`, applied
to each folder after it is scanned. A library built by a v0.2.0–v0.3.x version holds
ratings from that era's XMP-based source, which no INI has confirmed.

Nothing needs to be done: a normal rescan corrects a folder's stars the first time it's
walked again, whether that scan is manual or triggered by the file watcher. Deleting the
library (`photon/library.db`, in your user data directory) is not required — it only forces
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

### Camera data, keywords, people and albums

The viewer's ⓘ button (or `I`) opens an info panel: camera, lens, focal length, aperture,
shutter speed and ISO from the photo's EXIF; the keywords the photo carries in its XMP or
IPTC (as written by Picasa, Lightroom, Bridge, digiKam and the like); the people Picasa
named in the folder's `.picasa.ini`, outlined over the photo while the panel is open; and
checkboxes for photon's albums. `R` and `Shift+R` turn the photo on screen; nothing is
written, and the next photo opens upright.

The sidebar lists **Albums**, **People** and **Tags** above the years. Albums are photon's
own and live only in its library: create one with "New album…", add photos from a tile's
right-click menu or the info panel, and rename or delete from the album's right-click menu.
People and Tags are read from Picasa's INI and from the photos themselves and cannot be
edited here. The search box matches all of it: a camera or lens name, a keyword, `50mm`,
`f/1.8`, `iso400`, or a date such as `2024-06`.

Every word narrows the search: `italy lake` finds photos matching both, each word wherever
it likes (`lake.jpg` in the folder `2019 Italy`). `lake OR pond` widens it; `AND` and `OR`
count as operators only in capitals, so `salt and pepper` still looks for the word. Double
quotes make a phrase, and `camera:` or `lens:` confine a term to that field, so
`camera:canon 2019` is the Canon's photos from 2019 and not a folder named Canon. A camera or
lens in the viewer's info panel is a link to that search.

Two things to know. A library from an earlier photon picks up camera data and keywords on
the next scan of each folder, which reads every file's header once; nothing needs to be
done. And an album remembers photos by their library row, so a photo renamed or moved on
disk leaves its albums once the old row is purged — stars survive that because they live
in Picasa's INI, album membership does not.

### Rotating and cropping

In the viewer, `R` and `Shift+R` (or ↻ ↺) turn a photo, and `C` (or ✂) opens the crop tool:
drag the rectangle or its handles, pick a ratio to lock it, Enter applies, Escape cancels.
Cropping again shows the whole photo with the current rectangle, so you adjust it rather than
cropping what was left. **Original** undoes everything.

None of this changes your files. photon remembers the edit in its library and applies it
wherever it shows the photo — the grid, the viewer, a slideshow — so "Reveal in file manager"
still leads to the photo exactly as the camera wrote it, and other programs do not see the
edit. An edit belongs to the file's entry in the library: a photo renamed or moved outside
photon comes back unedited. Picasa's own crops and rotations are not imported.

### Duplicates

photon finds byte-identical files. After each scan it reads only the files that share their
exact size with another file, so on most libraries almost nothing is read. While any exist,
a **Duplicates** row in the sidebar shows every photo that has an identical copy, folder by
folder, and the viewer's info panel lists where a photo's copies are; clicking one locates it.
photon never deletes anything: use "Reveal in file manager" and decide there. Resized or
re-saved versions are different files and are not reported.

### Slideshow and fullscreen

Press `S` in the viewer (or the ▶ button) to play the current view from the photo on screen:
fullscreen, crossfading, looping back to the start at the end. Space pauses, the arrow keys
and the wheel step, the controls hide while the pointer rests, and Escape ends the show and
leaves you in the viewer on the photo it stopped at. How long each photo stays is set under
Settings → Slideshow.

`F11` toggles fullscreen at any time. photon remembers the window's fullscreen state, so if
you quit in the middle of a slideshow it reopens fullscreen; `F11` is the way out.

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
- Rust (stable, 1.88 or newer).
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
```

Build a release bundle for this OS with explicit host-appropriate bundles, e.g. on Linux
`npm run tauri build -- --bundles deb,appimage` (the release workflow selects the right
targets per platform: `dmg` on macOS, `msi` on Windows).

## Manual smoke checklist

Run this before each release, on each OS. The installed-app checks at the end can only be
done from a real installer, so run them against the draft release's artifacts before
publishing it.

- [ ] On first launch with a fresh profile, the Pictures folder is added and scanned without asking.
- [ ] Thumbnails appear within seconds, and scrolling stays smooth while indexing continues.
- [ ] In the viewer press `R`: within about half a second the photo is shown turned (preview first, then sharp), the caption's pixel size swaps, and back in the grid its tile is turned too. Press `R` four times quickly: it ends upright, not one turn short. Close and reopen photon: the turn is still there. The file on disk is unchanged (same size and date in the file manager).
- [ ] Press `C`: the whole photo shows with a bright rectangle and the rest dimmed; the eight handles resize, dragging inside moves, and nothing can be dragged off the photo. Choose 1:1: the rectangle becomes square *on screen* and stays square from every handle. Enter applies: the viewer shows only the crop, sharp at 100%, and the grid tile matches. `C` again shows the whole photo with the same rectangle. "Whole photo" then Enter removes the crop. Escape cancels without saving, and a second Escape is needed to close the viewer.
- [ ] Turn a cropped photo: the same part of the picture stays framed. With the info panel open on a photo with Picasa faces, the outlines still sit on the faces after a turn, and a face cropped out of the frame has no outline.
- [ ] "Original" appears in the bar only for an edited photo and restores it; its old thumbnail appears at once (it was still cached). A slideshow shows edited photos edited.
- [ ] Upgrade a real library from schema 8: no thumbnail is regenerated (the grid fills from cache as before).
- [ ] Copy a photo into another watched folder: within a scan the sidebar gains "⧉ Duplicates (2)", the view shows both files under their folders, and each one's info panel lists the other under "Identical copies"; clicking the path closes the viewer and lands on that copy. Edit or delete one of the two: after the next scan the row disappears.
- [ ] Upgrade a real library from schema 7: the first scans finish as quickly as before, and duplicates appear shortly after the status bar stops showing a scan.
- [ ] In the viewer, `S` starts a slideshow: the window goes fullscreen, photos crossfade (no flash of black between them) at the interval from Settings → Slideshow, and the last photo is followed by the first. Space pauses and resumes; ←/→ step and the next photo then stays a full interval; the bar, zoom and ✕ fade out after the pointer rests about 2.5 s and return when it moves. Escape ends the show, leaves fullscreen and stays in the viewer; a second Escape closes the viewer.
- [ ] Start a slideshow from a window that is already fullscreen (F11 first): ending the show leaves it fullscreen. F11 toggles fullscreen from the grid, the viewer and Settings.
- [ ] Settings → Slideshow: the field shows the stored interval; entering 0 or 999 snaps to 1 or 60; the next slideshow uses the new value.
- [ ] Double-click or Enter opens the viewer with an image in about 100 ms. The full resolution follows. ←/→, Home/End, Esc and Backspace work.
- [ ] The mouse's back side button closes the viewer, and does not navigate the page or leave a blank frame. Worth checking on each OS: the three webviews deliver side buttons differently, and some consume them for history before the page sees them.
- [ ] The gear at the right of the top bar opens Settings. Escape, the close button and a click outside the dialog close it, and focus returns to the gear.
- [ ] Settings → Folders lists every watched folder with its path, photo count and status, and the status shows scan progress live.
- [ ] "Add folder…" in Settings adds a folder. Adding a folder inside a watched one is refused with a clear message.
- [ ] Rescan works from Settings and from a sidebar folder's right-click menu. "Remove…" in Settings asks first, works during a scan, and leaves the files on disk.
- [ ] Settings → About shows the version matching the release tag, and its "Reveal" opens the folder holding `library.db`.
- [ ] With a few hundred tags, Settings → Tags stays centred in the window and the list scrolls inside the dialog.
- [ ] Settings → Tags lists every tag with its photo count, and the filter box narrows it. Rename a tag: Enter saves, Escape or clicking away cancels (Escape does not close Settings), a blank name is refused. The sidebar's Tags list and an open Tag view follow the new name.
- [ ] Renaming a tag to another existing tag asks to merge, then shows one tag whose count is the photos carrying either. "Remove…" asks first, and the tag disappears from the sidebar and from search.
- [ ] With the viewer's info panel open on a tagged photo, rename one of its tags in Settings: the panel shows the new name without reopening the photo, and the zoom and pan stay as they were.
- [ ] Rename a tag onto an existing one and decline the merge: the field stays open with what you typed.
- [ ] Each rename or removal appears under Changes, and "Restore" brings the original tag back. Rescanning the folder does not undo a rename, and the photo files' keywords are unchanged (check in another app).
- [ ] After removing every folder, the sidebar offers "Add a folder in Settings…", which opens Settings on Folders.
- [ ] Unplugging a drive with a watched folder dims its folder and tiles after a rescan. Nothing disappears.
- [ ] Ctrl/Cmd+Shift+R and "Reveal in file manager" open the system file manager at the file.
- [ ] The viewer's caption reads name, capture time, resolution, size and position, e.g. `IMG_1234.JPG · Jun 15, 2024, 12:30 PM · 4000 × 3000 · 3.2 MB · (12 / 240)`. The capture time matches what the camera wrote, not shifted by your time zone.
- [ ] Clicking the caption copies the file name to the clipboard and shows "Copied" for about a second. Paste somewhere to confirm.
- [ ] Right-click in the viewer: "Locate in photon" closes it and lands the grid on that photo, selected and in view — also from Starred, Recent or a search, where it switches back to All first. "Reveal in file manager" opens the file's folder. Escape closes the menu before it closes the viewer.
- [ ] A library that cannot be opened (for example a corrupt database, or one written by a newer photon) shows an error dialog and photon exits, with no empty window sitting behind it.
- [ ] A watched folder whose drive is offline and which has never been scanned appears in Settings → Folders as Offline, and can still be rescanned, revealed or removed from there.
- [ ] A release build (see Development above) starts and shows the library — not a blank window.
- [ ] If a scan removes photos while the viewer is open, the viewer shows "This photo is no longer available" rather than a blank frame, and Escape still returns to the grid.
- [ ] Open a photo, type a keyword in the info panel and press Enter: the chip appears at once, and the keyword shows in the sidebar's Tags list and finds the photo in search.
- [ ] Click the × on a keyword that came from the photo's own metadata: it goes from this photo only, and stays gone after the folder is rescanned.
- [ ] Rename that keyword in Settings → Tags: the photo's chip follows to the new name.
- [ ] With the viewer open on a photo, zoomed and panned, copying a photo into a folder that sorts *ahead* of it leaves the viewer on the same photo, at the same zoom and pan, with its caption renumbered. (The offset the viewer holds shifts when the grid is rebuilt; this is the check that it re-finds its photo rather than sliding onto the next one.)
- [ ] Copying a photo into a watched folder makes it appear in the grid within a few seconds, with no manual rescan.
- [ ] Deleting a photo on disk removes it from the grid.
- [ ] Renaming a folder on disk moves its photos in the tree within a few seconds.
- [ ] Unplugging a watched drive dims it; plugging it back in restores it within about a minute, unattended.
- [ ] Typing part of a folder's name in the search box finds its photos.
- [ ] The search box stays put at the top of the window while the folder list and the grid scroll.
- [ ] The bar between the folder list and the grid resizes the list along its full height: dragging it stops at a minimum width and at half the window, the grid reflows to the new width, and with the bar focused (Tab) the arrow keys resize it too. The list's lower-right corner no longer has a resize grip.
- [ ] Clearing the search box restores the full library.
- [ ] With photos from several years, a year strip shows right of the grid's scrollbar: years sit where they start, a line marks the current position and follows scrolling, hovering shows a year bubble, and pressing or dragging scrolls there — pressing on a printed year lands on that year's first folder, the same folder the sidebar lists first under it. The strip is absent in Recent, with a single year, and when everything fits on screen.
- [ ] `lake bell` shows only photos matching both words; `lake OR bell` shows photos matching either; typing `lake OR` on the way there keeps showing the `lake` results rather than flashing empty.
- [ ] In the viewer's info panel, click the camera (then, on another photo, the lens): the viewer closes, the search box reads `camera:"…"`, and the grid shows that camera's photos, the clicked one among them.
- [ ] Clicking a folder in the sidebar while a search is active leaves search and lands on that folder.
- [ ] Type a query, pause briefly, then keep typing without pausing again (e.g. type "beach", wait, then add "es" to make "beaches"): the box keeps every character you typed and never snaps back to an earlier, shorter query.
- [ ] Edit the search box, then immediately (within ~150ms) click a folder or Starred: the click's destination is what stays on screen — the grid must not jump back into a search a moment later.
- [ ] With Starred active and the search box empty, press Escape in the box: it stays on Starred rather than switching to All.
- [ ] With Starred or a search active, click a watched root (a bold top-level row) rather than a year row: it lands on that root's photos, the same as a year row does.
- [ ] Type text into the search box, then click a folder within about 150ms: the pending search is cancelled, so the box keeps the text (filtering nothing) until you clear it or press Escape.
- [ ] While a folder is being rescanned, the status bar shows "Scanning <folder>… N of ~M files (P%)" with a bar that fills, where M is the folder's photo count before the scan; a folder's first scan shows a moving indeterminate bar and a plain file count instead. Photos the scan adds are reported as "new or changed". The bar disappears when the scan ends.
- [ ] On a library large enough to exhaust the system's watch limit, the status bar says live updates are limited rather than silently missing changes.
- [ ] A folder added with "Add folder…" while photon is running picks up changes on disk within a few seconds, with no restart.
- [ ] Once a folder whose live updates were limited recovers, the status bar stops saying so without a restart.
- [ ] The downloaded installer runs, and photon starts from the installed location — not from a checkout.
- [ ] photon appears in the applications menu (Start menu, Launchpad) with the right name and icon.
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
- [ ] The tile of a starred photo shows a ★ badge, which appears and disappears with the toggle, and the sidebar's Starred count follows.
- [ ] Unstarring the photo on screen while Starred is showing keeps it on screen with no "n / m" in the caption; ArrowLeft goes to the previous starred photo, ArrowRight to the next, Escape returns to the grid.
- [ ] Clicking ★ while zoomed in toggles the star and does not start a pan.
- [ ] In the viewer, `R` turns the photo clockwise and `Shift+R` anticlockwise, as do the ↻ and ↺ buttons; a landscape photo turned on its side fits the window's height rather than being clipped, zoom and pan still work on the turned photo, and the next photo opens upright. No file's modification time changes.
- [ ] The ⓘ button (or `I`) opens the info panel with the camera, lens and exposure line for a photo from a camera, and "No camera data" for a screenshot. Esc still closes the viewer with the panel open.
- [ ] A photo with keywords written by Picasa, Lightroom or digiKam lists them under Keywords in the info panel, and each appears under Tags in the sidebar with a count; clicking a tag shows exactly those photos.
- [ ] A folder whose `.picasa.ini` names faces lists the names under People in the info panel, outlines each face over the photo while the panel is open (also after `R`), and lists the person in the sidebar's People group; clicking the person shows their photos. Naming a new face in Picasa appears within a rescan, without the photo changing.
- [ ] "New album…" in the sidebar takes a name on Enter and cancels on Escape or an empty name. A tile's right-click menu adds the photo to an album; in the album view it offers "Remove from". The info panel's checkboxes add and remove too, and the sidebar count follows. Rename and Delete… work from the album's right-click menu, and deleting the album on screen returns to All.
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
      is already the pinned theme, with no flash of the other.
- [ ] Pin a theme, then change the desktop's scheme: photon does not move.
- [ ] Checkboxes, the crop ratio menu and the scrollbars match the theme.
- [ ] Tab through the top bar, sidebar and Settings: every control shows the same blue
      focus ring, and opening the viewer or Settings draws no ring around the window.

## How watching works

photon watches each watched folder recursively for filesystem changes. When something
changes, it waits 2 seconds for things to settle, then rescans just the directories that
changed rather than the whole folder. If the OS won't grant a watch (for example the
system's watch-descriptor limit is exhausted), that folder falls back to a full rescan
every 5 minutes instead, and the status bar shows "Live updates limited" while any folder
is in that state.

## Licence

MIT. See [LICENSE](LICENSE).
