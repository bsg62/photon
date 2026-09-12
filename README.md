# photon

A fast, local photo manager for Linux, macOS and Windows. A spiritual successor to Picasa 3.

photon watches folders in place. It never moves or changes your files. It keeps a small
SQLite library and a thumbnail cache in your user data and cache directories.

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
```

Build a release bundle for this OS with `npm run tauri build`.

## Manual smoke checklist

Run this before each release, on each OS:

- [ ] On first launch with a fresh profile, the Pictures folder is added and scanned without asking.
- [ ] Thumbnails appear within seconds, and scrolling stays smooth while indexing continues.
- [ ] Double-click or Enter opens the viewer with an image in about 100 ms. The full resolution follows. ←/→, Home/End and Esc work.
- [ ] "Add folder…" adds a folder. Adding a folder inside a watched one is refused with a clear message.
- [ ] Rescan works. "Remove from photon" asks first, works during a scan, and leaves the files on disk.
- [ ] Unplugging a drive with a watched folder dims its folder and tiles after a rescan. Nothing disappears.
- [ ] Ctrl/Cmd+Shift+R and "Reveal in file manager" open the system file manager at the file.
- [ ] A library that cannot be opened (for example a corrupt database, or one written by a newer photon) shows an error dialog and photon exits, with no empty window sitting behind it.
- [ ] A watched folder whose drive is offline and which has never been scanned still appears in the sidebar as a dimmed row, and can still be rescanned or removed from there.
- [ ] A release build (`npm run tauri build`) starts and shows the library — not a blank window.
- [ ] If a scan removes photos while the viewer is open, the viewer shows "This photo is no longer available" rather than a blank frame, and Escape still returns to the grid.
- [ ] Copying a photo into a watched folder makes it appear in the grid within a few seconds, with no manual rescan.
- [ ] Deleting a photo on disk removes it from the grid.
- [ ] Renaming a folder on disk moves its photos in the tree within a few seconds.
- [ ] Unplugging a watched drive dims it; plugging it back in restores it within about a minute, unattended.
- [ ] On a library large enough to exhaust the system's watch limit, the status bar says live updates are limited rather than silently missing changes.

## How watching works

photon watches each watched folder recursively for filesystem changes. When something
changes, it waits 2 seconds for things to settle, then rescans just the directories that
changed rather than the whole folder. If the OS won't grant a watch (for example the
system's watch-descriptor limit is exhausted), that folder falls back to a full rescan
every 5 minutes instead, and the status bar shows "Live updates limited" while any folder
is in that state.
