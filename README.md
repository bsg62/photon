# photon

A fast, local photo manager for Linux, macOS and Windows. A spiritual successor to Picasa 3.

photon watches folders in place. It never moves or changes your files. It keeps a small
SQLite library and a thumbnail cache in your user data and cache directories.

## Install

Download the installer for your system from the [latest release](https://github.com/bsg62/photon/releases/latest).

| System | File | Install |
|---|---|---|
| Linux | `.AppImage` | `chmod +x photon_*.AppImage && ./photon_*.AppImage` |
| Linux (Debian, Ubuntu) | `.deb` | `sudo apt install ./photon_*.deb` — pulls in webkit2gtk-4.1 and libsoup-3 |
| macOS | `.dmg` | Open it and drag photon to Applications. Take the `aarch64` file for Apple Silicon, `x64` for Intel. |
| Windows | `.msi` | Run it. |

### Upgrading to a version with Starred photos

photon reads star ratings from each photo's embedded XMP as it indexes it. A library built
by an earlier version has no ratings recorded, and photon will not re-read a file whose size
and modification time have not changed — so Starred would stay empty.

Delete the library and let photon rebuild it. It lives in your user data directory
(`photon/library.db`); your photos are untouched, since photon never writes to watched
folders. Rebuilding re-reads every photo, ratings included.

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
- Rust (stable, 1.88 or newer). `mise use rust@stable` works.
- Node.js 24 or newer (an LTS release; the UI toolchain — Vite 8, Vitest 5 and
  `@sveltejs/vite-plugin-svelte` — requires Node 22.12+, 24+ or 26+, so a plain "Node 22" install
  can be too old depending on its exact patch version. Node 24 is the current LTS line and is
  used in CI).
- The [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/) for your OS. On Linux that means webkit2gtk-4.1, libsoup-3 and librsvg2.

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
- [ ] Double-click or Enter opens the viewer with an image in about 100 ms. The full resolution follows. ←/→, Home/End, Esc and Backspace work.
- [ ] The mouse's back side button closes the viewer, and does not navigate the page or leave a blank frame. Worth checking on each OS: the three webviews deliver side buttons differently, and some consume them for history before the page sees them.
- [ ] "Add folder…" adds a folder. Adding a folder inside a watched one is refused with a clear message.
- [ ] Rescan works. "Remove from photon" asks first, works during a scan, and leaves the files on disk.
- [ ] Unplugging a drive with a watched folder dims its folder and tiles after a rescan. Nothing disappears.
- [ ] Ctrl/Cmd+Shift+R and "Reveal in file manager" open the system file manager at the file.
- [ ] A library that cannot be opened (for example a corrupt database, or one written by a newer photon) shows an error dialog and photon exits, with no empty window sitting behind it.
- [ ] A watched folder whose drive is offline and which has never been scanned still appears in the sidebar as a dimmed row, and can still be rescanned or removed from there.
- [ ] A release build (see Development above) starts and shows the library — not a blank window.
- [ ] If a scan removes photos while the viewer is open, the viewer shows "This photo is no longer available" rather than a blank frame, and Escape still returns to the grid.
- [ ] Copying a photo into a watched folder makes it appear in the grid within a few seconds, with no manual rescan.
- [ ] Deleting a photo on disk removes it from the grid.
- [ ] Renaming a folder on disk moves its photos in the tree within a few seconds.
- [ ] Unplugging a watched drive dims it; plugging it back in restores it within about a minute, unattended.
- [ ] Typing part of a folder's name in the sidebar search box finds its photos.
- [ ] Clearing the search box restores the full library.
- [ ] Clicking a folder in the sidebar while a search is active leaves search and lands on that folder.
- [ ] Type a query, pause briefly, then keep typing without pausing again (e.g. type "beach", wait, then add "es" to make "beaches"): the box keeps every character you typed and never snaps back to an earlier, shorter query.
- [ ] Edit the search box, then immediately (within ~150ms) click a folder or Starred: the click's destination is what stays on screen — the grid must not jump back into a search a moment later.
- [ ] With Starred active and the search box empty, press Escape in the box: it stays on Starred rather than switching to All.
- [ ] Type text into the search box, then click a folder within about 150ms: the pending search is cancelled, so the box keeps the text (filtering nothing) until you clear it or press Escape.
- [ ] On a library large enough to exhaust the system's watch limit, the status bar says live updates are limited rather than silently missing changes.
- [ ] A folder added with "Add folder…" while photon is running picks up changes on disk within a few seconds, with no restart.
- [ ] Once a folder whose live updates were limited recovers, the status bar stops saying so without a restart.
- [ ] The downloaded installer runs, and photon starts from the installed location — not from a checkout.
- [ ] photon appears in the applications menu (Start menu, Launchpad) with the right name and icon.
- [ ] A freshly installed photon watches the Pictures folder on first launch, with no folder added by hand.
- [ ] On macOS, the permission prompt for the Pictures folder appears. Denying it shows "Live updates limited" in the status bar rather than silently indexing nothing.
- [ ] On Linux, `sudo apt install ./photon_*.deb` pulls in the webview dependencies on a clean machine, and photon starts rather than failing on a missing library.
- [ ] The security warning each OS shows matches what the README's "photon is not code-signed" section says to expect.
- [ ] `sha256sum -c SHA256SUMS --ignore-missing` passes against the downloaded files.
- [ ] On a freshly built library, photos rated in Picasa show under Starred with a matching count, once the first scan has finished.
- [ ] Clicking Starred shows only starred photos; clicking any folder returns to the full library at that folder.
- [ ] A library carried over from v0.2.0 shows no stars until it is deleted and rebuilt, as the README's upgrade note says.
- [ ] After a full scan, no photo file's modification time has changed — photon reads ratings and never writes them.

## How watching works

photon watches each watched folder recursively for filesystem changes. When something
changes, it waits 2 seconds for things to settle, then rescans just the directories that
changed rather than the whole folder. If the OS won't grant a watch (for example the
system's watch-descriptor limit is exhausted), that folder falls back to a full rescan
every 5 minutes instead, and the status bar shows "Live updates limited" while any folder
is in that state.

## Licence

MIT. See [LICENSE](LICENSE).
