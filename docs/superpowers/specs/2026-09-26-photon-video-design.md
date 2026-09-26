# Video

2026-09-26

Video was deferred on 2026-09-12 (`2026-09-12-photon-plan3-watcher-design.md` §1), when the
assumption was that it arrives with ffmpeg. That assumption predates the rule that photon ships
no native libraries, and it was never tested. This design exists because it was: the webview
already has a video decoder on every platform, and a spike measured what it takes to feed it.

The goal is Picasa's: videos sitting next to photos in a watched folder, typically a phone's
camera roll, appear in the grid in capture order with a poster frame, and play in the viewer.

## The spike, and what it settled

Probed 2026-09-26 in a throwaway program (wry 0.55.1, the version in `Cargo.lock`, so the same
custom-scheme path Tauri uses on Linux) against WebKitGTK 2.52 and GStreamer 1.28, headless on
a GTK Broadway display. Findings:

- **`<video src="photon://…">` cannot work on Linux.** WebKitGTK hands the media URL to a
  GStreamer `playbin`, and WebKit's source element takes only http(s) and blob URLs. The load
  fails 12 ms in with `loadingFailed: FormatError` before a pipeline exists, whatever the
  scheme handler answers - ranged, whole, with or without CORS. `file://` fails the same way.
  The only request the handler ever sees is a 1446-byte MIME sniff.
- **Two carriers work.** A `blob:` URL filled by `fetch`ing `photon://`, which holds the whole
  file in memory and is useless for a video of several GB; and **a loopback HTTP server**
  (`http://127.0.0.1:<port>`) with `crossorigin="anonymous"` on the element and
  `Access-Control-Allow-Origin` on the response. Without that header CORS refuses the load,
  as it should.
- **A frame drawn to a canvas stays readable** over the loopback server: `getImageData` and
  `toDataURL` succeed. A poster frame can therefore be made by the webview.
- **An open-ended range must be answered to its end.** A server that caps `bytes=N-` at about
  1 MB - which is what Tauri's own asset protocol does - broke every far seek on 100 MB and
  276 MB files (`downloadbuffer: buffer offset does not match current writing position`,
  then "Could not demultiplex stream"); the small clips passed only because they fit in what
  was already buffered. Answering to the end of the file, streamed, fixed both: the 276 MB 4K
  H.264 file seeked to 90% in 329 ms, the 100 MB Theora file in 29 ms. The client hanging up
  mid-body is the normal end of most requests.
- **A stock Arch/Omarchy install cannot play video, and crashes trying.** `webkit2gtk-4.1`
  lists `gst-plugins-good`, `-bad` and `gst-libav` only as optional, and this machine had none:
  no MP4 demuxer, no H.264 or HEVC. Worse, a `<video>` load without `gst-plugins-good` fails to
  find `autoaudiosink` and **aborts WebKit's web process (SIGABRT)** - in photon that is the
  whole window going blank, not one failed tile. `canPlayType` answered `""` for MP4 and did
  not crash.
- **With the plugins present everything plays.** Unpacked into the scratchpad and pointed at
  with `GST_PLUGIN_PATH`, H.264 in MP4 and MOV, HEVC (`hvc1` MOV, iPhone-style), VP9, AV1 and
  Theora all played, seeked, and drew a readable frame. First frame 34-210 ms.
- **`preload="auto"` spools the whole file** to `/var/tmp/WebKit-Media-*`. A poster frame at
  1 s from the 4K file took 200-300 ms either way.

macOS (WKWebView) and Windows (WebView2) were not measured; see *Risks*.

## Decisions

**Playback is the webview's.** photon adds no decoder. Codec coverage is therefore the
platform's: macOS plays H.264 and HEVC; Windows plays H.264, VP9 and AV1, and HEVC only with
Microsoft's HEVC Video Extensions and hardware support; Linux plays whatever GStreamer plugins
are installed.

**Poster frames are made by the webview too** (chosen over per-OS thumbnail APIs and a bundled
ffmpeg). One implementation, no new dependency, and coverage exactly equal to playback's: a
video photon cannot thumbnail is one it could not play either. Rejected:

- *Per-OS APIs* (AVFoundation, the Windows shell thumbnail API, `gstreamer-rs`) would keep the
  work in the backend's pool, but they are three FFI implementations, two testable only in CI,
  and a GStreamer build dependency on Linux.
- *A bundled ffmpeg sidecar* covers every codec but costs ~80 MB per platform and LGPL/GPL
  notices, and abandons the no-native-libraries rule for video.

The cost accepted: video poster frames fill in only while the window is open.

**Media travels over a loopback HTTP server on all three platforms**, not `photon://` on two and
HTTP on one. Plain HTTP with ranges is what every webview plays best, and one path is one set of
tests.

**The slideshow skips videos.** It stays a photo slideshow: no interaction between the slide
timer and playback, and nothing starts making noise mid-show.

## Scope

`MediaKind::Video` covers `.mp4`, `.m4v`, `.mov` and `.webm`. AVI, MKV and 3GP are left out:
the webviews cannot reliably play them, and a tile that will not play is worse than no tile.

## Data and scanning

**Schema 20** adds one column, `items.duration_ms INTEGER` (NULL for photos). `kind` has been
in `items` since schema 1 and `MediaKind::to_db` gains `Video => 1`; `from_db` accepts it. The
`.unwrap_or(MediaKind::Image)` reads in `library/items.rs` stay, now reachable only by a value
from a newer photon, which `SchemaTooNew` already refuses. The schema bump makes the release a
minor. Nothing is backfilled: videos have never been rows, so the first scan after the upgrade
indexes them as new files, and `EXIF_VERSION` is not bumped because no existing row gains a
field.

**`photon_core::video`** reads metadata in pure Rust: a hand-rolled ISO-BMFF box walker, in the
manner of the XMP and INI parsers, for MP4 and MOV, and a minimal EBML reader for WebM. It never
decodes a frame. From MP4/MOV it takes `mvhd` (duration, creation time), the video track's
`tkhd` (dimensions and the rotation matrix; the video track is the one whose `hdlr` is `vide`),
and `moov/meta` `keys`/`ilst` for `com.apple.quicktime.creationdate`,
`com.apple.quicktime.make` and `com.apple.quicktime.model`. From WebM it takes the Segment
Info's duration and the video track's pixel dimensions. It is bounded like every other parser
here: a box larger than its parent, a zero-size box that is not the last, or a depth past a
small limit ends the walk with what has been read, never a panic or a loop.

**The capture date**, first that exists, each passed through `plausible_taken_at`:

1. Apple's `creationdate` (`2024-06-15T12:30:45+0200`): its wall-clock part, read as
   naive-as-UTC - which is exactly what `taken_at` holds for a photo, so a video sorts among
   the photos taken beside it.
2. `mvhd.creation_time`, seconds since 1904 in UTC, where 0 means none: converted to wall-clock
   time with the system time zone *at that instant* (`jiff`, pure Rust, `TimeZone::system()`),
   then read as naive-as-UTC. Android writes only this; without the conversion its videos sort
   hours away from its photos. It is the machine's zone, not the camera's - a video shot abroad
   is placed by home time - which is the best the file allows.
3. The file's mtime, as for a photo without EXIF.

WebM has no dependable capture date and goes straight to the mtime.

**Rotation is resolved at scan time.** `width`/`height` are stored as the video *displays*,
swapped for a 90° or 270° `tkhd` matrix, and `orientation` is always 1. The grid's aspect logic
is unchanged, and nothing downstream rotates a frame the browser has already rotated.

`describe()` branches on the kind: `read_image_meta` and `read_embedded` for a photo,
`video::read_meta` for a video (camera make and model filled for iPhone videos, everything else
in `CameraMeta` empty; no keywords, rating or caption). Picasa's per-folder pass applies stars,
hidden flags and albums to videos unchanged - Picasa itself did.

**Search** gains the terms `video` and `photo`, matching on kind, in `search::Query` beside the
existing prefixed terms. Camera search reaches iPhone videos through make and model.

## The media server

`photon-app/src/media_server.rs`, on `tiny_http` (pure Rust, synchronous), so photon owns no
HTTP parser. It binds `127.0.0.1:0` - loopback only, a port the OS picks - and is started in
`setup` beside `engine.startup`, and answers each request on a thread of its own, holding an
`Arc<Engine>` - not a fixed pool, because a playing or paused `<video>` holds its connection
open and tiny_http writes the body with no write timeout, so a fixed pool is exhausted by that
many videos and every later request waits.

**One route:** `GET` and `HEAD` `/<token>/video/<id>`. The id is looked up with
`engine.lib.item`; it must be a live row (`missing_since IS NULL`) of kind `Video`, or the
answer is 404. Hidden videos are served, because the Hidden view shows them. No part of the URL
ever becomes a filesystem path. The file is opened read-only, keeping "no watched photo is ever
opened for writing" true for videos.

**Ranges.** A single range - `a-b`, `a-`, or the suffix form `-n` - is answered `206` with
`Content-Range`, **to its own end, never capped**, streamed from the file in 64 KB reads. No
`Range` header gets a `200` of the whole file, also streamed. An unsatisfiable range gets `416`;
a multi-range request gets the whole file as a `200`, which HTTP permits. `Content-Type` comes
from the extension; `Accept-Ranges: bytes` is always sent. A client that hangs up ends the
request quietly - it is how the player moves on after a seek, not an error to log.

**Security.** A loopback port is reachable by anything running on the machine, and by a web
page in the user's browser through DNS rebinding, so:

- a **128-bit token**, random per launch, is the first path segment, compared in constant time;
  a wrong token is a `404`, not a `403`, so the server does not confirm it exists;
- the **`Host` header must equal `127.0.0.1:<port>`**, which is what defeats rebinding - a
  rebound page sends its own host name;
- `Access-Control-Allow-Origin` is the app's exact origin (`tauri://localhost`;
  `http://tauri.localhost` on Windows), never `*`, and responses carry `Cache-Control: no-store`;
- there is no other route, no listing, and nothing but video items.

**Wiring.** A new IPC command `media_base() -> String` returns `http://127.0.0.1:<port>/<token>`
(the three files, plus an answer in `mock.js`); `url.ts` gains `videoUrl(id)` built on it. The
CSP gains `media-src 'self' http://127.0.0.1:*`. `photon://` keeps serving images and
thumbnails as today.

## Poster frames

**Videos never reach the decode pool.** `pending_thumb_ids` gains `kind = 0`, and the worker's
`process` refuses a video row, so no worker ever calls `decode_image` on one. Videos instead have
a **video job list** in the engine, ordered with the existing `Priority`: `Background` for every
pending video after a scan, `Visible` when the thumbnail protocol is asked for a video's
thumbnail. The protocol waits on the same completion signal it waits on for a photo, so a tile's
`<img>` resolves the moment its frame lands. The engine emits a `videoJobs` event when the list
goes from empty to non-empty.

**`createVideoThumbnailer`** (`ui/src/lib/video-thumbnailer.svelte.ts`), mounted once in
`App.svelte`, with its video element, canvas and timers injected so it is tested with fakes. One
job at a time - a WebKit media pipeline is heavy:

1. `api.nextVideoJob()` returns `{ id, key }` or null; on null it sleeps until `videoJobs`.
2. It loads a hidden, muted `<video preload="metadata" crossorigin="anonymous">` from
   `videoUrl(id)`.
3. It seeks to `min(1 s, duration / 10)` - the first frame is often black.
4. It draws the frame at up to the preview's 1600 px long edge, encodes a JPEG, and sends it
   with `api.putVideoFrame(id, key, bytes)` as a raw IPC body, not base64.
5. A media error, or 15 s without a frame, sends `api.videoFrameFailed(id, reason)`.

After each job it unloads the element (`removeAttribute('src')`, `load()`), releasing the
pipeline.

**`put_video_frame` treats the bytes as untrusted.** They are decoded with `image` as JPEG only,
within a dimension bound, and go through the same resize-and-store path as `ThumbCache::render`'s
output. The row is marked `Ready` with `set_thumb_state_if_unchanged` against the `key` the job
was issued with, so a video rewritten while its frame was being made is refused, as a stale
worker's render is. A video's thumbnail key is its bare fingerprint - videos have no edits.

**Failure reasons.** `unsupported` (`MEDIA_ERR_SRC_NOT_SUPPORTED`, or the capability check
below says no) leaves the row `Pending` and skips it for the rest of the session, so installing
codecs later fills it in, at the cost of one failed load - about 12 ms - per launch. `decode` and
a timeout mark it `Failed`, like a broken photo.

**The crash-loop guard.** A video can take WebKit's web process down, and with it the UI - the
spike showed exactly that. `next_video_job` records the id as in flight through
`thumbs/inflight.rs`, and a put or a fail clears it. An id still in flight at the next launch
counts a death; at `DEATHS_TO_FAIL` it is `Failed`, so relaunching never replays the same crash.

**Duplicates.** Videos are **excluded** from the look-alike pass - both `CANDIDATES_SQL` and
`percep_hashes` in `library/similar.rs` gain `kind = 0`. A poster frame is not the video, and it
would pair with the still taken beside it. Videos are **included** in content hashing: a
byte-identical copy of a video is a real duplicate, and candidates remain same-size only.

## The capability check

At startup the UI evaluates `mediaSupport = canPlayType('video/mp4') !== ''`. Until it is true,
no `<video>` element is ever created: the thumbnailer takes no jobs and the viewer does not
play. On Linux this is the guard against the crash, not merely a feature check: GStreamer's MP4
demuxer ships in `gst-plugins-good`, the same package as the `autoaudiosink` whose absence
aborted the web process, so "MP4 is supported" proves the crash cannot happen. The code carries
that reasoning, so nobody replaces the MP4 probe with a codec that lives in a different package.

Without support the viewer shows the poster frame, or a film placeholder, and says the system
cannot play videos; on Linux it names GStreamer's good and libav plugins and links the README.

## The UI

**Types.** `GridEntry.kind` becomes `'image' | 'video'` and gains `durationMs`; `ViewerItem`
gains `kind` and `durationMs`. The Rust structs, the TS mirror and every literal in
`library.test.ts` change in the same commit.

**Tile.** A play badge and the duration (`m:ss`) in a corner - a Lucide `play` icon from
`lucide-static`, colours from tokens. A video whose frame is not made yet shows a film
placeholder rather than a broken image.

**Viewer.** For a video, `<video controls preload="metadata" crossorigin="anonymous"
poster={preview}>` takes the full `<img>`'s place, and **plays on open, with sound**, as Picasa
did. Leaving it - navigation, closing, or a reload through `pictureChanged` - pauses it and
unloads the source. There is no zoom, pan or crop; `R` and `C` do nothing for a video. **Space**
toggles playback (Space is otherwise bound only while a slideshow runs). Arrows, Home, End and
Escape keep their viewer meaning: key handling stays on the viewer root and the video element is
not made focusable. `neighbours` leaves videos out of the preload. The info panel shows the
duration and dimensions, and the camera rows when present.

**What refuses a video.** The slideshow skips videos, and does not start in a view holding only
videos. `rotate_item` and `set_item_edit` refuse a video in the backend as well as the UI.
`copy_picture` refuses one, and the menu hides the item. Export copies bytes and works unchanged;
reveal, open in the default app, star, hide, albums, keywords and captions are unchanged. Faces:
none - Picasa wrote none for videos.

## Packaging

- `.deb`: `depends` gains `gstreamer1.0-plugins-good` and `gstreamer1.0-libav`.
- AppImage: `bundleMediaFramework: true`, which bundles GStreamer and its plugins; the size cost
  and the plugins' licences go in `THIRD-PARTY-NOTICES.md`.
- `tiny_http` and `jiff` join `THIRD-PARTY-NOTICES.md`; `xtask metadata` enforces it.
- README: *File formats* gains Video, including Windows needing the HEVC extension for iPhone
  video and Linux needing the plugins for anything but the AppImage; the smoke checklist gains
  playback, seeking, poster frames, the Linux no-plugins message, and the macOS/Windows items
  under *Risks*.

## Testing

- **`photon_core::video`**, on small hand-built byte fixtures (no encoder in the test path):
  each capture-date source and their precedence; a zero `mvhd` time; the time-zone conversion
  under a fixed zone, not the system's; a 90° `tkhd` swapping the dimensions; the video track
  found behind an audio track; WebM duration and size; truncated, oversized, zero-size and
  deeply nested boxes ending the walk without a panic.
- **Scanner**: videos indexed with kind, duration and date; the `video`/`photo` search terms;
  videos absent from both look-alike queries and present in content hashing, each pinned.
- **Media server**: `a-b`, `a-`, `-n`, unsatisfiable → 416, multi-range → 200; an open-ended
  range answered to the end of a file larger than any chunk size (the spike's bug); a wrong
  token, a wrong `Host`, a photo id and a missing row all 404; the exact ACAO origin; a client
  hanging up mid-body.
- **Poster frames**: `put_video_frame` rejects a non-JPEG and a stale key, and marks `Ready` and
  wakes a waiting thumbnail request; the worker pool never takes a video; an in-flight video
  counts a death on the next launch.
- **`createVideoThumbnailer`**, with fakes and fake timers: the seek time for short and long
  clips; the 15 s timeout; `unsupported` skipped while `decode` fails; no job taken while
  `mediaSupport` is false; the element unloaded after every job.
- The viewer's and tile's wiring is effect code - `svelte-check` and the smoke checklist cover
  it, and the commit says so.
- **Screenshots**: new `SHOTS` for a grid holding videos and the viewer on one; every new command
  answered in `mock.js`.

## Risks

- **macOS and Windows are unmeasured.** App Transport Security is expected to exempt a loopback
  IP; neither platform's firewall is expected to prompt for a loopback-only listener; WebView2
  is expected to allow `http://127.0.0.1` media from `http://tauri.localhost`. Each is a
  blocking smoke-checklist item for the release, not an assumption.
- **HEVC on Windows** depends on a paid Store extension and hardware support. Without it an
  iPhone video stays `Pending` with no poster and does not play; that is documented, not solved.
- **Codec coverage differs by platform**, so one library can show a poster on one machine and
  a placeholder on another.

## Not in this design

Trimming, rotating or otherwise editing video; extracting a still; audio-only files; AVI, MKV
and 3GP; slideshow playback; making poster frames without the window open; GPS from `©xyz`
(photon has no map); XMP inside MP4; motion photos and Live Photos (the video half of a Live
Photo is indexed as a separate video).
