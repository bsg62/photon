# Plan: Settings, the dialog (sub-project 6, PR 1)

Spec: `docs/superpowers/specs/2026-10-15-photon-native-pages-design.md`. Port of
`ui/src/components/Settings.svelte` and `ui/src/lib/settings.ts`, the sections this pull
request builds: Appearance, Slideshow, Duplicates, People, About. Folders and Tags (PR 2),
Statistics and Shortcuts (PR 3) are in the section list and show their heading.

1. `settings.rs`, a state module: `Section` and the nine sections' names; the words of each
   section (the Svelte component's, held to it by a test that reads it); `Settings` - the
   section shown, what has been read of the stored settings (interval, shuffle, look-alike
   distance; none until read, so no control shows a value that is not the stored one), the
   interval's field (digits only; stored at Enter, at leaving the field and at closing,
   clamped by the library and shown as stored), the People row's question (asked only when
   the library says people are named), the memory figure; `Asked`, what a frame of the dialog
   asks for; `memory_amount`, `memory_scope` (native words: no web view).
2. `settings_view.rs`: the dialog's body - header with its title and cross, the section
   list, the section scrolled by a number of its own (`PANEL_NOTCH`'s forty points a notch),
   nothing out of sight handed to egui. Segmented controls (theme, size, look-alikes), the
   interval field, the checkboxes, the question in the row with the keyboard on Cancel.
3. `dialogs.rs`: `Dialog::Settings`. `dialog_view.rs`: its size (720 by 520, less the
   window's margin), `Done::Settings(Vec<Asked>)`.
4. `app/settings.rs`, the application's side: opening (gear, sidebar's button, empty
   library's "Watched folders…"), reading the stored values off the thread, the memory every
   two seconds while About is shown, the writes (`Write::Theme`, `Interval`, `Shuffle`,
   `Distance`, `FaceDetection`) and their answers, the face summary read before switching off.
5. The theme put on the window at once, egui's visuals and the title bar
   (`ViewportCommand::SetTheme`), at launch as at a change (`theme::apply::put`).
6. Tests: the state module's; frame tests of the view; the whole application - the gear
   opens Settings, a theme chosen is stored and shown, the interval stored as typed and
   clamped, look-alikes stored, Find faces off with a named person asks first and Cancel
   keeps them, About's figures. Each probed.
7. CLAUDE.md paragraph; gate; PR; CI on three systems; review; one fix pass; merge.
