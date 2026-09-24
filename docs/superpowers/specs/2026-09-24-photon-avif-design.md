# AVIF photos

2026-09-24. Approved in conversation the same day.

## What it is

`.avif` files in a watched folder are indexed like any other photo: thumbnail, viewer, EXIF
(date, camera, keywords), edits, export, duplicates and look-alikes. The user's AVIFs may come
from anywhere, so both kinds are in scope: an exported or downloaded single image, and a phone
camera's tiled ("grid") photo, possibly 10-bit.

## Why it is not two feature flags, as TIFF and BMP were

`image`'s `avif` feature only *encodes* (ravif). Decoding is `avif-native`, which is dav1d, a
C library, and photon takes no native dependencies. Every other pure-Rust AVIF decoder found
on 2026-09-24 was ruled out:

- `zenavif`, `rav1d-safe` (and `heic`) are AGPL-3.0 or commercial; photon is MIT.
- `avif-parse` 2.1 has no grid images and no `irot`/`imir`, so phone photos would decode as
  one tile, unrotated.
- `gamut-avif` has no AV1 decoder of its own yet.

What remains is two crates:

- **`rav1d` 1.1** (BSD-2-Clause), the memory-safety port of dav1d, for the AV1 decoding. It is
  built with `default-features = false, features = ["bitdepth_8", "bitdepth_16"]`, which drops
  its hand-written assembly and with it the need for `nasm` and a C compiler on the three CI
  runners. It builds on stable Rust; its own `rust-toolchain.toml` pins a nightly, but that
  only applies when building rav1d on its own, not as a dependency.
- **`zenavif-parse` 0.6** (MPL-2.0, dependencies MIT/Apache-2.0), for the container: primary
  item, alpha item, grid tiles and layout, `irot`, `imir`, `clap`, colour information and
  resource limits. It has no `unsafe` code. It comes from the same vendor as the AGPL crates,
  but this crate carries its own MPL-2.0 licence.

**Measured on 2026-09-24**, with a throwaway probe using both crates, release build, one decode
thread, no assembly: a 4000×3000 single-image AVIF (12-bit) in **180 ms**, the same picture
as a 10-bit 2×2 grid in **300 ms**, parse included. No assembly needed.

Both are ordinary crates.io dependencies, not vendored material, so `THIRD-PARTY-NOTICES.md` is
unchanged, as for every other crate in the tree. MPL-2.0's file-level copyleft is satisfied
by the crate's source being unmodified and published.

## Decisions

- **The viewer is shown the file itself.** `/image/<id>` serves the unedited AVIF as it serves
  a JPEG, with `mime_for` answering `image/avif`. WebView2 and macOS 13+ decode it natively,
  with their own fast decoder and HDR handling. Where the webview cannot (macOS 10.15-12, and
  WebKitGTK builds without AVIF), the full-size image's `decode()` fails and the viewer stays
  on the 1600px preview, as it already does for any full image that fails to load. Rendering
  to JPEG on the backend for every open was rejected: a full-size decode per photo, serialised
  behind `RENDERING`, and AVIFs dropping out of the neighbour preload, all to help older
  platforms that still get a 1600px picture. A UI probe of webview support that asks for a
  backend render only where needed stays possible later without undoing any of this. An
  *edited* AVIF already goes through `edit::render_full` and so shows full-size everywhere.
- **One place decides how a file is decoded.** `decode.rs` gains `open_image(path)`, which
  reads the leading `ftyp` box (capped at 256 bytes) and sends a file whose major *or
  compatible* brands include `avif` or `avis` to the AVIF decoder, and everything else to
  `ImageReader::with_guessed_format` as today. Its three callers are `decode_oriented`,
  `edit::render_full` and `metadata::read_header`'s dimensions. Content-sniffed, not by
  extension, like every other format here.

  `image`'s decoding hooks were considered and rejected. Its format guess sees only the first
  16 bytes, so a file whose major brand is `mif1` is not recognised. `ImageReader::open` then
  takes the `.avif` extension as the built-in `ImageFormat::Avif`, which skips the hooks and is
  refused as unsupported. It is also global mutable state that every test entry point would
  have to remember to register.
- **The container's transforms are applied in the decoder, and EXIF orientation is ignored
  for AVIF.** In HEIF/AVIF the `irot`/`imir` properties are what rotate the picture; browsers
  apply them and ignore EXIF orientation. So `decode_avif` applies `clap` (crop), then `irot`,
  then `imir`, in the order MIAF specifies, and `describe()` keeps `orientation = 1` for an
  AVIF even when its EXIF says otherwise. The dimensions stored are the displayed ones. With
  orientation 1 that still satisfies "dimensions before orientation", and thumbnails, the
  webview's rendering of the raw file and face rectangles agree. Honouring EXIF orientation
  as well would rotate a phone photo twice.
- **No `EXIF_VERSION` bump, no migration.** As with TIFF (PR #67): `describe()` is unchanged for
  every existing format, and a `.avif` was never indexed, so there is no stale row. The next
  walk adds them as new files. The AVIF branch in `describe()` only changes behaviour for AVIF
  files, and there are none yet.
- **Everything downstream is unchanged.** Look-alikes and duplicates read the cached thumbnail
  or the file's bytes. Export copies bytes. The thumbnail key does not depend on format.

## The decoder: `photon_core::avif`

- **`avif/av1.rs`** holds the only `unsafe` code: a wrapper over rav1d's exported functions
  (`dav1d_default_settings`, `dav1d_open`, `dav1d_data_create`, `dav1d_send_data`,
  `dav1d_get_picture`, `dav1d_picture_unref`, `dav1d_close`, reached as
  `rav1d::src::lib::*` with types from `rav1d::include::dav1d::*`). The context and the picture
  each get a guard whose `Drop` releases them, so an error path cannot leak either. Every
  `unsafe` block gets a `// SAFETY:` comment. Settings: `n_threads = 1` and
  `max_frame_delay = 1`, because the thumbnail pool already decodes `MAX_WORKERS` photos at
  once, and rav1d's default of one thread per core on top of that would oversubscribe the
  machine. `EAGAIN` from `get_picture` means "send more"; with the whole item already sent,
  hitting it again means the item held no frame, which is an error. The output is the planes
  (Y, U, V; 8-bit or 16-bit samples), bit depth, chroma layout, and the sequence header's
  colour description and range.
- **`decode_avif(bytes) -> Result<DynamicImage>`**, in order:
  1. Parse with `DecodeConfig::default()`, whose tile and animation-frame caps apply on this
     (lazy) parse path. Its `peak_memory_limit`/`total_megapixels_limit` do **not** apply here
     - in zenavif-parse 0.6.2 those two are enforced only by the eager, deprecated path
       (behind its `eager` feature, which photon does not enable) - so they are not set. What
       actually bounds an AVIF decode is rav1d's own per-frame pixel cap
       (`av1::MAX_FRAME_PIXELS`, the same bound `image`'s default limits give every other
       decode) plus, for a grid, a check in `grid()` of the declared canvas against that same
       bound before allocating it - a container-declared grid output size is otherwise
       unbounded and unchecked before the allocation that uses it.
  2. Decode the primary item, or every grid tile.
  3. Convert YUV to RGB. Matrix coefficients come from the container's `colr` (nclx) box when
     present, otherwise from the AV1 sequence header, and default to BT.601 when both are
     unspecified. BT.709, BT.601 and BT.2020 non-constant-luminance are supported, plus
     identity (RGB coded as GBR). Limited or full range. Chroma 4:2:0, 4:2:2, 4:4:4 and
     4:0:0, with the chroma sample nearest each pixel (no upsampling filter: this feeds a
     thumbnail, and the viewer shows the webview's own decode). Samples above 8 bits are
     rounded down to 8 bits.
  4. Decode the alpha item, if any, into an RGBA8 image. Premultiplied alpha
     (`premultiplied_alpha()`) is divided out.
  5. Stitch grid tiles row by row and trim to the grid's declared output size.
  6. Apply `clap`, `irot`, `imir`.

  Errors are `image::ImageError::Decoding` with format hint `avif`, so they arrive at callers
  as the same `Error::Image` every other broken photo does, and a bad AVIF gets the
  failed-thumbnail placeholder without stalling its folder.
- **`avif_dimensions(bytes) -> Option<(u32, u32)>`** answers from the container alone, with
  no AV1 decode: the grid's output size or, for a single image, the AV1 sequence header's
  `max_frame_width`/`max_frame_height` (not the `ispe` property) - equivalent to it for a
  still, since the two differ only when the bitstream's `frame_size_override_flag` is set,
  which a still-image encoder does not write - after `clap`, with width and height swapped
  for `irot` 90/270. This is what `read_header` stores.
- **A grid's rows, columns and output size come from the grid item's own ImageGrid payload**
  (`AvifParser::primary_data()` on a grid item), parsed by photon's `grid_layout` - not from
  `AvifParser::grid_config()`. In zenavif-parse 0.6.2, `grid_config()` only reads an
  `ImageGrid` *property* box, which no real file carries: HEIF stores the ImageGrid as the
  grid item's own data. Lacking that property, it falls back to dividing the primary item's
  `ispe` by a tile's `ispe`, and only when that division is exact; otherwise it reports
  `rows = tile_count, columns = 1, output 0x0`. A grid padded to the tile size - the ordinary
  phone-camera shape, whose declared output is not an exact multiple of the tile size - hits
  that fallback and would be decoded as a vertical stack of full, unpadded tiles instead of
  a canvas trimmed to its real dimensions. `grid_layout` parses the payload directly instead:
  `version` (u8, must be 0), `flags` (u8, bit 0 selects u32 vs u16 output fields),
  `rows_minus_one`, `columns_minus_one` (u8 each), then `output_width`/`output_height`.
- The conversion is plain Rust, not the `yuv` crate: one more dependency buys SIMD speed for a
  step that costs a fraction of the AV1 decode.

## Wiring

- `MediaKind::from_path`: `"avif"`.
- `protocol.rs` `mime_for`: `Some("avif") => "image/avif"`.
- `metadata::read_header`: dimensions through `open_image`'s routing, and the AVIF check that
  pins `orientation` to 1.
- README: AVIF in the supported-formats list, with its limits, and smoke-checklist items:
  - A phone AVIF (grid, rotated) shows upright in the grid and the viewer, with the same
    framing in both.
  - The viewer at 100% on Windows, macOS and Linux. On a platform whose webview cannot show
    AVIF, the viewer stays on the preview, with no error.
  - An AVIF with transparency.
  - A turn or crop of an AVIF.

## Limits, stated rather than hidden

- **HDR is not tone-mapped.** A PQ or HLG AVIF is reduced to 8 bits as if it were SDR, so its
  thumbnail may look flat or dim. When a file carries a gain map, the SDR base image is what
  photon shows, which is the intended fallback.
- **Animated AVIF** shows its primary still image. A sequence with no primary item gets the
  failed-thumbnail placeholder.
- **`clap` in the webview.** Whether every webview applies the clean aperture is not certain,
  so on the rare file that has one, the full-size view could frame slightly differently from
  the thumbnail. A smoke-checklist item, not something the code can settle.
- **Decode cost.** Roughly 0.2-0.3 s per 12 MP photo, once, when its thumbnail is rendered.
  Enabling rav1d's assembly later is a feature flag plus `nasm` on the CI runners, if that ever
  matters.
- **A decoder panic aborts photon.** Every other format's decoder panic is caught by the
  thumbnail service's `catch_unwind` and costs one thumbnail, but rav1d's crates.io release
  exposes only `pub unsafe extern "C" fn`s, and a panic cannot unwind out of an `extern "C"`
  boundary (it aborts the process instead, since Rust 1.81); there is no other, safe-to-unwind
  entry point to call instead. That abort, an allocation failure, and the OOM killer would
  otherwise crash-loop photon on the same photo at every launch, since none of them run `Drop`
  for `catch_unwind` to contain. `thumbs/inflight.rs` guards against the loop rather than the
  abort, keyed by the photo rather than the id: a marker file per in-flight thumbnail decode,
  under the cache, is turned into a death record - `"<count> <thumb_key hex>"` - the moment the
  service next starts, and the marker is deleted in that same pass, so a death is counted
  exactly once no matter how many launches follow it. A record only ever matches the photo's
  *current* `thumb_key()`, so a reused SQLite id, a changed file or a fresh edit never inherits
  another photo's deaths. A photo whose record reaches two deaths is failed with
  `CRASH_MESSAGE` instead of being decoded a third time; one death alone is forgiven, since
  quitting, a power cut or an unrelated crash while a photo happened to be in flight would
  otherwise blame it. A suspect (one recorded death) decodes under an exclusive lock that
  every other decode only takes shared - acquired *before* the photo is even marked in
  flight, not after, so a photo merely queued behind the suspect's turn never has a marker on
  disk for a decode that has not actually started, and only the actual culprit can ever reach
  two. The marker is always cleared once a decode finishes, whatever it decided, but the death
  record only when it decided the photo's fate one way or another (rendered, or explicitly
  failed): a transient failure - the drive dropped out, the cache went unwritable - decides
  nothing and leaves the item `Pending` for a retry, so a suspect's earlier death has to
  survive it. A deliberate quit disarms the guard first (`ThumbService::close`), which the
  real shutdown path (`RunEvent::Exit` -> `Engine::shutdown` -> `close()`, then tauri's own
  `process::exit`, without joining the worker threads) requires to do the removing itself
  rather than trust a worker to finish and clean up after itself. A full-size render of an
  edited photo and export can still abort photon this same way, but only when the user asks
  for one, so neither can loop.

## Testing

AV1 bitstreams cannot be built by hand the way the TIFF and BMP fixtures were, and encoding
them in the test would need `image`'s `avif` encoder (rav1e) as a dev-dependency. So small
fixtures made with `avifenc` are committed under `crates/photon-core/testdata/avif/`, with a
`README.md` giving the exact command for each. Each is a few kilobytes, drawn in two or four
flat colours so that position and colour can be asserted, not just size:

| Fixture | Proves |
|---|---|
| 8-bit 4:2:0, red left / blue right | the plain path, the matrix and range (colours within a tolerance) |
| 10-bit 2×2 grid, four quadrant colours | tiles stitched in the right order, output trimmed, >8-bit reduced |
| 2×2 grid padded to a non-tile-multiple output (129×129 from 65×65 tiles) | `grid_layout` reads the ImageGrid payload's declared output directly, so a phone-camera-shaped grid is trimmed to its real size instead of stacked as an unpadded N×1 column |
| `irot` 90 (`avifenc --irot 1`) | rotation applied, dimensions swapped in `avif_dimensions` too |
| `imir` | mirroring applied on the right axis |
| alpha | RGBA out, alpha values kept |
| 4:0:0 monochrome | the grey path |
| 4:4:4 full range | the other range and layout |
| major brand `mif1`, `avif` compatible | `open_image`'s sniff reads compatible brands |
| EXIF orientation 6 (`avifenc --exif`) | `describe()` stores orientation 1, and EXIF date and camera still come through |

Plus a scanner test that a `.avif` beside a `.jpg` is indexed with the right dimensions and a
`Ready` thumbnail, and `protocol.rs`'s MIME test extended to `.avif`.

Each test is watched failing with its change reverted, per CLAUDE.md. Removing the `mif1`
compatible-brand read, the `irot` step, the tile order or the EXIF-orientation pin must each
fail a named test, and the red/blue fixture must fail if the matrix is swapped (BT.601 for
BT.709). Two red/blue colours alone would not catch that swap, so that fixture uses a colour
that the two matrices map apart by more than the tolerance.
