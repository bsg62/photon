# Exporting copies

## Why, and what it changes about photon's promise

photon has no way to get a photo *out*. Every other verb works inside the library: star, tag,
album, edit. "Send these twenty to someone" has no answer but the file manager, and in the file
manager the user has to know which twenty.

This is the first feature that writes a photo file. The promise it must not break is the one
in Conventions: **photon never writes to, moves or deletes the photos it watches.** Export does
not touch them either — it writes *new* files, at a destination the user picks, and refuses a
destination inside a watched root. A spec-level decision, taken 2026-09-20; the Conventions
bullet gains a sentence.

## What the user sees

Right-click a selection in the grid → **Export copies…**. A dialog:

- The destination, chosen with the system folder picker (`dialog:allow-open`, already granted).
- A checkbox, **Apply edits to the copies**, remembered between exports in the `settings`
  table. Ticked, an edited photo is re-encoded as it is shown (turns and crop applied);
  unticked, every photo is copied byte for byte and edits stay inside photon.
- A line under the checkbox, always visible, because it is the one surprise in the feature:
  *A re-encoded copy carries no camera information.* photon reads EXIF and never writes it
  (no native dependencies, so there is no writer to reach for), so an edited photo that is
  re-encoded loses its make, model, lens and capture date. An **unedited** photo is a byte
  copy in both modes and keeps everything — so the loss is confined to photos the user
  deliberately edited, which is why the box is ticked by default.
- Export / Cancel.

The status bar reports progress (`34 / 120`), and a toast reports the result: *Exported 118 of
120 photos*, with the reason for the shortfall if there is one.

## Decisions

**A destination inside a watched folder is refused**, the way adding a watched folder inside
another one is. Exported copies there would be scanned back in as new photos — the user would
have doubled their library with one click, and the duplicate finder would then report every
pair.

**Nothing is ever overwritten.** A name already taken — on disk, or by a file this same export
wrote a moment ago (two folders can each hold `IMG_1234.JPG`) — gets ` (2)`, ` (3)` before the
extension. The two sources are checked together: the filesystem alone would let one export
collide with itself.

**A rendered copy is JPEG at quality 95, not the viewer's 92.** `render_full`'s doc says its
render is "for looking at, never for keeping", and an exported copy is exactly the thing it
says it is not. The quality becomes a parameter of the shared renderer and that comment is
corrected rather than left to mislead the next reader. A source with transparency stays PNG,
as it does in the viewer.

**One file at a time.** Full-size decode and encode is bounded by `RENDERING` in `protocol.rs`
for the same reason: a parallel export of twelve 60-megapixel photos is twelve full decodes in
memory at once. Export takes the same lock, so a user browsing during an export still gets
their picture and neither path can starve the other of memory.

**No thread.** The loop runs inside a `#[tauri::command(async)]`, which Tauri already puts on
a worker, so the UI is not blocked; a long export occupies one worker of the pool for its
duration. A thread of its own buys nothing until there is something to cancel.

**No cancellation in v1**, and no resume. Recorded here rather than hidden: the dialog says how
many photos it is about to write before it starts, which is the point where the user can still
change their mind.

**A photo that cannot be read is skipped, not fatal** — one unreadable file must not cost the
user the other 119. The count in the toast is what landed, and the first reason is reported.
The same rule `set_stars` and the batch keyword writers already follow.

## Surface

```
photon_core::export::{Plan, copy_or_render}      # headless: naming, collisions, the render
Engine::export_items(ids, dest, apply_edits) -> ExportReport    # a loop, + progress events
Engine::check_export_dest(dest)                  # the refusal, asked when the folder is picked
events::ExportProgress { done, total, failed }   # trait method + Tauri impl + Recorder
commands::export_items -> ExportReport { written, failed, reason }   # + ipc.rs + app.rs
settings::export_apply_edits / set_export_apply_edits
```

UI: `createExportDialog` in `lib/export-dialog.svelte.ts` (destination, checkbox, the busy
guard, the message wording), `ExportDialog.svelte` for markup and focus, a `SHOTS` entry, and
answers in `mock.js`.

## Tests

- `export.rs`: a name already on disk gets ` (2)`; two sources with one name inside a single
  export get ` (2)` as well (the case the filesystem check alone misses); an unreadable source
  is skipped and counted; an edited photo comes out with the edit applied and the right
  dimensions; an unedited photo comes out byte-identical to its source in **both** modes.
- `engine.rs`: a destination inside a watched root is refused before anything is written;
  progress events add up to the total; the photo files in the watched folder are untouched
  (mtimes and bytes) after an export.
- `export-dialog.svelte.test.ts`: no destination means no write; the double-submit guard; the
  wording, including the short count.
