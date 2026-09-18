# photon — Non-Destructive Edits Design

**Date:** 2026-09-18
**Status:** Approved design, implemented
**Builds on:** v0.14.2, schema 8 → 9

## 1. Scope and the promise

Quarter turns, a crop, and "back to the original". Straightening, colour adjustments and
importing Picasa's own `rotate=`/`crop=` lines were considered and not chosen; nor was export,
so an edit is visible only inside photon. The promise is untouched: **photon never writes to a
photo file.** An edit is two columns in `library.db`, applied on the way to the screen.

## 2. One definition

`photon_core::edit::Edit { turns: 0..=3, crop: Option<Crop> }`: EXIF **orientation**, then
clockwise quarter **turns**, then a **crop** of the turned picture — the picture the user was
looking at when they drew it. `Crop` is four 16-bit fractions (`n / 65535`), packed into one
integer column the way Picasa packs a `rect64`: exact in the database, stable in a hash, and
unchanged by a trip through JSON. `Edit::new` is the only door for untrusted parts: turns wrap,
an inverted or sliver crop (under ~1% a side) is refused, and a whole-frame crop is no crop, so
"untouched" has one representation.

`Edit::turned` carries the crop round with the picture. The test for it is a property, not an
example: rendering the turned edit equals turning the rendered edit, pixel for pixel, through
four turns and back.

## 3. The backend renders; the UI draws nothing

The alternative — CSS transforms in the viewer — would have put the geometry in the one
component that cannot be tested here, and a second copy of it in the thumbnailer. Instead the
edit is applied in exactly three places, all Rust, all pixel- or value-tested:

1. **Thumbnails.** `ThumbCache::render(source, orientation, edit)`. A crop is taken from the
   full-size decode and only then shrunk; shrinking first would crop an already small preview
   (a quarter of the frame at half the preview's resolution). A turn alone keeps the fast path.
2. **The full image.** `/image/<id>` serves the file for an untouched photo, exactly as
   before, and `edit::render_full` for an edited one (JPEG q92, or PNG when the source has
   alpha). `/image/<id>/uncropped` applies only the turn: the crop tool draws on it.
3. **Faces.** `viewer_item` maps Picasa's rectangles through `Edit::map_rect`; a face whose
   centre is cropped away is dropped.

For an edited photo `ViewerItem` reports `width`/`height`/`orientation` *as shown* (edited
size, orientation 1), because that is the picture every URL serves, plus `uncroppedWidth/Height`
for the crop tool and `edit` itself. `GridEntry.aspect` follows the edit.

Cost, accepted: opening an edited photo full-size is a decode and an encode per request
(about half a second for 24 MP); the preview thumbnail is on screen meanwhile, as always.
Untouched photos pay nothing. An edited animated GIF is served as a still, since the render
decodes one frame.

## 4. The thumbnail key

`Edit::thumb_key(fingerprint)`: the bare fingerprint for an untouched photo — every thumbnail
cached before this feature stays valid, and resetting an edit finds the original's thumbnail
still cached — otherwise XXH3 of fingerprint, turns and crop. Consequences, each pinned:

- `Item::thumb_key()` replaces `Item::fingerprint()`; `map_grid_row` and `live_fingerprints`
  derive the same key from columns.
- `set_item_edit` sets the row `Pending`, and bumps the thumbnail GC epoch in its transaction
  (the fourth orphaning writer; the tripwire test now names four). The same edit again writes
  nothing.
- `set_thumb_state_if_unchanged` compares the edit too: a worker that rendered the pre-edit
  picture must not mark the row `Ready` with nothing cached under the new key.
- The engine queues the photo at `Visible` priority and rebuilds the grid; the viewer's
  existing rebind effect sees a new `thumbKey` and reloads. An edit reaches the screen the way
  a file changed on disk does. An edited photo's full-image URL carries `?k=<thumbKey>`,
  since the path does not change with the edit and an `<img>` given the URL it has does not
  refetch.

`update_items` leaves the edit alone: a photo re-saved by another program is still the photo
the user turned. Edits are by item id, so a file renamed outside photon loses its edit with its
old row, like album membership — recorded, not fixed.

## 5. The viewer

`R`/`Shift+R` were display-only turns that reset on navigation; they now call `rotate_item`,
and the display-rotation machinery (`Rotation`, `rotated`, `isQuarterTurn`, `.frame.quarter`) is
gone. `rotate_item` is serialised in the engine because it reads the edit it builds on: commands
run on a pool, and two quick presses would otherwise come out as one turn.

`C` opens the crop tool on the whole turned picture with the current crop drawn, so cropping
again adjusts rather than compounds. `crop.ts` holds the geometry and `crop-tool.svelte.ts` the
state; both are tested. Two rules worth keeping:

- A drag is recomputed from the rectangle at pointer-down, never accumulated per move: what the
  picture's edge swallows of one move would otherwise be lost to the next.
- A ratio preset is a ratio of *pixels*; the rectangle is in *fractions*, so the picture's own
  ratio is divided out (`fractionRatio`), and presets turn to face a portrait photo.

The tool owns the keyboard while open (Enter applies, Escape cancels, nothing navigates), closes
only after a successful write, and is cancelled when the loader moves to another photo.

## 6. Testing

Rust: the edit model (nine tests, pixel-level), storage and key derivation, the GC tripwire, the
stale-worker guard, crop-before-shrink, the service end to end, protocol and `viewer_item`. Ten
mutation probes, ten failures. UI: `crop.ts` and the crop tool, six probes, six failures. The
overlay's CSS was measured in headless Chromium (handles centred on the corners and hittable).
What remains is component wiring — five README checklist lines, including the schema 8 → 9
upgrade regenerating no thumbnails.

## 7. What the branch review found

An independent read of the branch before merge found one bug the feature armed and three
weaknesses; all four are fixed in the second commit.

- **An unrelated change reloaded an edited photo, closing the crop tool.** The viewer reloads
  when a re-read shows a different picture, and "different" compared `thumbState` plainly. An
  edit sends the row to `pending`; the viewer re-reads it before the worker finishes; the next
  library change of any kind delivered `ready` and reloaded the photo - mid-crop if a scan
  finished then. The comparison predates edits but only ever fired for a just-scanned photo.
  Now `picture.ts` (`pictureChanged`, tested): the state counts only across `failed`.
- **A stale thumbnail URL could be cached for a year with the wrong picture.** The thumb handler
  serves the photo's current thumbnail whatever key the URL carries, as `immutable`. Harmless
  while keys never recurred; "Original" and a fourth turn bring a key back. It now answers
  `no-store` unless the URL's key is the current one.
- **Full-size renders were unbounded, and preloads wasted them.** `neighbours` no longer offers
  edited photos for the full-image preload (the viewer asks for them under a keyed URL, so the
  preload's render was never read), and `protocol.rs` renders one at a time: a render holds a
  whole decoded photo outside the thumbnail pool that bounds decode memory.
- **A turn could overwrite a reset.** `edit_write` serialised turns only against each other;
  `set_item_edit` now takes it too. No test: the race has no seam, and the lock is a leaf.

Recorded, not fixed: on a panorama beyond about 10:1, a locked ratio can produce a rectangle
under the backend's 1% minimum. The save is refused with `invalidCrop` and the tool stays open.
