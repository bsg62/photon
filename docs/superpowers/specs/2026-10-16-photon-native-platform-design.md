# photon native UI - platform and packaging

Sub-project 7 of `2026-10-09-photon-native-ui-design.md`: "Window state, the themed title
bar, fullscreen, file dialogs, the clipboard, reveal and open-in-app, AccessKit switched on,
the fallback for a machine without a usable GPU, and the installers built without Tauri's
bundler (two `.dmg`, `.deb`, AppImage, `.msi`; the MSI's opt-in desktop shortcut is a copied
Tauri WiX template today and has to be rebuilt). The version's authority moves from
`tauri.conf.json` to `Cargo.toml`, and `xtask versions` with it."

Written 2026-10-16, on the user's word ("continue with project 6 and then 7. Do as you see
fit"), with three of their answers given before they went: **the GPU fallback is wgpu's own
software adapter** (WARP on Windows, llvmpipe on Linux; a plain message where even that
fails; no second renderer), **file dialogs on Linux go through the XDG desktop portal
only** (rfd over D-Bus, no GTK; the typed path stays where no portal answers), and **the
installers are built by a workflow on `native-ui` as artifacts, never as a release** - main's
Tauri release is untouched until the switch-over (sub-project 8).

## What is there already

The theme is put on the system's title bar at launch and at every choice, and put back
when Windows takes it away (`theme::apply::put`, `keep_title_bar`, sub-project 6). F11 and
the slideshow's fullscreen are built (sub-project 4). What is left of those two is the
smoke checklist's: whether each system's title bar follows, which no test can see.

Everything else this sub-project builds stands in the window already, in its place and
taking no press, as the sub-projects before it drew it: "Choose…" beside the export's and
Settings' typed paths; "Copy photo" in the tile menu; "Reveal in file manager" in the tile,
viewer and folder menus, on each watched folder's row and on the library's row in About;
"Open in default app" in the tile and viewer menus; "Open in OpenStreetMap" in the info
panel, drawn as dim words. Each of them starts to work in its place, and nothing moves.

## Decisions

1. **The window's own state is eframe's to keep** - its size, its place, maximized and
   fullscreen (`NativeOptions::persist_window`, eframe's `persistence` feature), in a file
   beside `library.db` (`persistence_path`: `window.ron`), not in `layout.json` as the
   sidebar spec once said. eframe already does what the Tauri plugin does and what photon
   would otherwise rebuild: it puts a window back only where a monitor still is
   (`clamp_position_to_monitors`), and on Wayland, where a window has no place it can ask
   for, it keeps the size alone. egui's own memory is not kept (`persist_egui_memory`
   off): what photon remembers is in the library and in `layout.json`, and a second
   record of the same things would disagree with them. A slideshow quit in the middle
   reopens fullscreen, as the Tauri photon does (`WINDOW_STATE_FLAGS` keeps
   `FULLSCREEN`), and F11 is the way out. The window gets photon's icon
   (`ViewportBuilder::with_icon`, from the PNG the installers carry).

2. **Folders are chosen through rfd**, the crate Tauri's dialog plugin is built on and
   already in the lockfile: on Linux with its `xdg-portal` feature alone (ashpd over zbus,
   pure Rust, no GTK - "No system library dependencies beyond the web view" holds), on
   Windows and macOS the system's own. "Choose…" beside the export's destination and beside
   Settings' "Add folder…" field opens it; the folder chosen is put in the field and asked
   about as a typed one is, so the two ways are one rule. The empty library's "Add
   folder…" opens the picker straight away, as the Svelte one does, and Settings with its
   field only where the picker could not be opened. **The picker runs off the window's
   thread** (rfd's async dialog, polled by a task): a portal answers when the user does,
   and a window that stopped drawing meanwhile would be reported as not answering. Where no
   portal answers (a desktop without `xdg-desktop-portal`), the picker says so once and the
   typed path stays - it is not taken away anywhere.

3. **The questions photon asks stay photon's own dialogs.** The Svelte UI asks through the
   system's `ask`; the native window has a question dialog of its own that is tested,
   themed and keyboard-safe (`Confirm`, Settings' questions in their rows). A system
   message box is used only where photon has no window to draw in: **the library could not
   be opened** (Tauri's `message` of "photon could not open its library", after the window
   is hidden) and **no adapter at all** (decision 6). On Linux rfd shows a message through
   `zenity` where it is installed, and through nothing where it is not, so each is also
   written to the standard error and the log.

4. **The clipboard is arboard**, which Tauri's clipboard plugin wraps and the lockfile
   holds: pure Rust on every system. "Copy photo" and Ctrl/Cmd+C, in the grid with one
   photo selected and in the viewer, copy the picture as shown - `commands::copy_picture`,
   the edit applied, at most 2,560 pixels a side, under `RENDERING` - off the window's
   thread, with "Photo copied" when it is done, and a second press while one is on its
   way doing nothing, as the Svelte store has it. A video is no photo to copy: the item is
   dimmed. A click on the viewer's caption copies the file name, as the Svelte caption
   does. On Linux, arboard hands the picture to the clipboard owner the system has; on
   X11 the picture lives only as long as photon does (the clipboard there is the owning
   program), which is how every X11 program's copy works and is said nowhere.

5. **Reveal and Open are photon's own, small, per system** (`platform::reveal`,
   `platform::open_path`, `platform::open_url`), the same calls Tauri's opener plugin makes:
   Windows `SHOpenFolderAndSelectItems` on an `ILCreateFromPathW` of the path, which the
   `windows` crate already in the build has; macOS `NSWorkspace
   activateFileViewerSelectingURLs` through `objc2`, which the AVFoundation backend
   already uses; Linux the file manager's D-Bus interface
   (`org.freedesktop.FileManager1.ShowItems`, through zbus, already in the lockfile), and
   where no file manager answers, the folder opened. Open in default app and a URL are
   `ShellExecuteW`, `NSWorkspace openURL:` and the portal's `OpenURI` (`xdg-open` where no
   portal answers). **Every stored path is `paths::canonicalize`'s already**, so a share on
   Windows reaches the shell as `\\server\share\...`, the form it takes. Each call runs off
   the window's thread - a file manager on a sleeping share can take seconds - and a
   failure is a message with the system's words. Ctrl/Cmd+Shift+R reveals the selected
   photo, as the Svelte grid does. Over a video that cannot be played the viewer offers
   "Open in default app", the way out the video spec left for here.

6. **The GPU's fallback is wgpu's software adapter, chosen by photon from what wgpu lists**
   (`WgpuSetupCreateNew::native_adapter_selector`; egui-wgpu does not expose
   `force_fallback_adapter`): the adapter egui-wgpu would choose when there is one, and
   a CPU adapter - WARP, llvmpipe - only when no other can draw to the window's surface.
   A window drawn by the CPU says so once in the status bar's place for messages ("Drawn
   without a graphics card: photon will be slow"), since every frame then costs what a
   GPU's would not. Where even that fails, `run_native` returns an error, and the error is
   decision 3's message - not a panic, not a silent exit. The GL backend stays as egui-wgpu
   has it (`PRIMARY | GL`), so a machine whose Vulkan is broken and whose GL works draws
   by GL first.

7. **AccessKit is switched on** (eframe's `accesskit` feature). Every row, button, face and
   link has told it what it is since sub-project 1, which is how the tests read the window;
   what switching it on adds is the system's side - a screen reader can now reach them.
   Two things it brings that the tests could not see: **a screen reader's press goes
   through egui's `Modal`** (a dialog's ground keeps the pointer from what lies under it,
   not an AccessKit action), so the dialog layer drops the AccessKit actions aimed under
   it, as it drops a button going down; and the custom-drawn tiles name their photo by its
   file name and its place. Not attempted: a full audit with a screen reader, which is the
   smoke checklist's.

8. **The installers are built by `xtask package`**, one subcommand for each, run by a
   workflow on `native-ui` (`native-packages.yml`: on a push to `native-ui` that touches
   the packaging, and by hand) that uploads them as artifacts and makes no release. The
   build of the artifacts is Rust in xtask, so the files it writes - the `.desktop` file,
   `Info.plist`, the Debian `control` file, the WiX sources - are tested as text in the
   gate, where the workflow only runs the tools:
   - **macOS**: `photon.app` (the binary, `Info.plist` with the identifier
     `io.github.bsg62.photon`, `icon.icns`), signed ad hoc (`codesign -s -`, which an arm64
     binary needs to run at all) and made into a `.dmg` by `hdiutil`, once for arm64 and
     once for x86-64 (`avif-asm` on x86-64 only, as the release has it).
   - **Linux**: the `.deb` by `dpkg-deb` from a tree xtask writes - `usr/bin/photon`, the
     `.desktop` file, the icons in `hicolor` - with `Depends` on the libraries the binary
     links (GStreamer's core and its plugins-base libraries among them, checked against
     `dpkg-shlibdeps`), and `Recommends` on GStreamer's good plugins and libav, the
     portal, and Mesa's Vulkan drivers. The AppImage by `linuxdeploy`, which bundles the
     libraries the binary links and never GStreamer's plugins: video in the AppImage uses
     the host's (`AppRun` points GStreamer's plugin path at the usual places distributions
     put them), and where it finds none the viewer says what is missing, as it does on a
     Linux without the plugins. Built on the oldest Ubuntu the runners have, for glibc.
   - **Windows**: the `.msi` by WiX 3 (as the Tauri bundler uses), from photon's own
     `main.wxs` - rebuilt from the copied template, which names Tauri's variables
     throughout - keeping what the copied one does: the opt-in desktop shortcut
     (`DESKTOPSHORTCUT`, a checkbox in the install-dir dialog, `msiexec ...
     DESKTOPSHORTCUT=1` unattended), the Start menu shortcut, the upgrade code the Tauri
     MSI has so the native one replaces it, and the workflow's check that the shortcut is
     opt-in, read from the MSI's own tables.
   - **The version's authority is the workspace's `Cargo.toml`** (`[workspace.package]
     version`), and `xtask versions` checks the others against it - `tauri.conf.json` and
     `ui/package.json` until sub-project 8 deletes them. `xtask metadata` checks the
     native packages' metadata (the identifier, the categories, the copyright, the
     licence) in place of the bundle section it reads today.
   - **A Windows "N" edition starts.** The `windows` crate links Media Foundation's DLLs
     when the program loads, so where the Media Feature Pack is missing photon-native
     cannot start at all. Those DLLs are delay-loaded (`/DELAYLOAD`, `delayimp.lib`), and
     `video::System::can_play` asks whether `mfplat.dll` can be loaded from System32 before
     anything of Media Foundation is called: where it cannot, every video says the system
     cannot play it, as a Windows without the HEVC extension says of HEVC. The workflow
     checks with `dumpbin /imports` that the DLLs are delay-loaded; that photon starts
     without them is the smoke checklist's.
   - `THIRD-PARTY-NOTICES.md` gains what the native build adds - egui's fonts, the
     binding crates' licences - checked by `xtask metadata` against the lockfile's licence
     fields for the crates it names.

   Unsigned, as the Tauri installers are.

## The pull requests

1. **The window**: its state through eframe (decision 1) and its icon. This spec rides in it.
2. **Folder pickers and the messages with no window** (decisions 2 and 3): "Choose…" in
   the export and in Settings, the empty library's "Add folder…", the library that cannot
   be opened.
3. **The clipboard** (decision 4): Copy photo in the grid, its menu and the viewer; the
   caption's file name.
4. **Reveal and open** (decision 5): every item in its place, Ctrl+Shift+R, the video's
   way out, OpenStreetMap.
5. **AccessKit and the GPU's fallback** (decisions 6 and 7).
6. **The installers** (decision 8): `xtask package`, the workflow, the version's authority,
   the delay-loaded Media Foundation, the notices.

Each is reviewed by a fresh reader before it merges, as every pull request of the branch has
been, and each new test is shown to fail with its rule broken.

## Testing

What a frame can show is tested in frames, as before: a press on "Choose…", "Copy photo",
"Reveal" reaches a seam (`App::with_platform`, a stand-in for the picker, the clipboard and
the file manager, as `App::with_videos` stands in for the system's video) and the test reads
what was asked of it and what the window then says. The real calls are not run by the gate:
a portal, a clipboard owner and a file manager are things a CI runner has none of. The
adapter's choice is a pure function over a list of adapters' descriptions, tested so. The
packages' text files are tested as text; the packages themselves are built only by the
workflow, and opened by nobody but a person - each system's install, start and uninstall
goes on the smoke checklist with the title bar, the screen reader and the N edition.

## Not in this sub-project

The parity checklist and the smoke checklist rewritten, the screenshot harness, the
website's images, deleting `ui/` and `photon-app`, and a release: sub-project 8.
