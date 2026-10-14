# The native window's video (sub-project 5)

2026-10-14. Sub-project 5 of `2026-10-09-photon-native-ui-design.md`, whose "Video" section
took the decision this one carries out: the operating system's decoders, one `video` module
with one trait, three implementations - AVFoundation, Media Foundation, GStreamer - and each
player plays its own sound. The behaviour to match is `2026-09-26-photon-video-design.md`'s,
less what the web view did for it (the loopback media server, the webview thumbnailer, the
capability check of `canPlayType`).

The user's rulings for this sub-project, given before it began (2026-10-14): **the decision to
drop a platform is mine**, from the spike, written here; **H.264 is required and HEVC is
best-effort** - an HEVC file the system cannot decode keeps its cached poster or a
placeholder and opens in the system's player; and **Linux video is tested on CI only**: the
machine this is built on has GStreamer's core and none of its decoders, and no system package
is installed on it, so a test that needs a decoder says so and passes over itself where none
is.

## The spike, and what it settled

A throwaway program (`spike/video` on the branch `native-ui-video-spike`, never merged) asked
each system's player, on each hosted runner, for: a poster frame at a time; the frames of four
seconds of playback with the sound on, as RGBA; and the first frame after a seek. The files
were made by ffmpeg for it: four seconds of red, green, blue and white, a second each, with an
AAC track, as H.264 and as HEVC (`hvc1`, as an iPhone writes it); one stored a quarter turned
(left half red, right half blue, with a 90-degree display matrix); and one of 1920 by 1080.
Measured 2026-10-14:

| | GStreamer (Ubuntu 24.04) | AVFoundation (macOS 15, arm64) | Media Foundation (Windows Server) |
|---|---|---|---|
| H.264: poster, playback with sound, seek | all | all | all |
| HEVC | all, with `gst-libav` | all | none: no decoder on the runner |
| First poster / next | 119 ms / 47 ms | 888 ms / 90 ms | 594 ms / 19 ms |
| Seek to a frame | 57 ms | 135 ms | 2.1 s (the engine's start) |
| 1080p, handed over as RGBA | 30.5 frames a second | 26 | 29.5 |
| A quarter-turned file | poster and frames turned | poster turned, frames not | frames turned, poster not |
| No sound device (the runners) | plays; ALSA complains | plays | plays |

**No platform is dropped.** Every backend gives what photon needs for H.264. HEVC is there on
two of three, as ruled.

What the table says beyond yes and no, each of which the backends answer:

- **Rotation is each backend's to finish.** GStreamer turns both, by `videoflip
  video-direction=auto`. AVFoundation's image generator turns the poster
  (`appliesPreferredTrackTransform`); its player's video output hands frames over as stored,
  so the backend turns them by the video track's `preferredTransform`. Media Foundation's
  Media Engine turns what it plays; its source reader, which makes the poster, says the turn
  (`MF_MT_VIDEO_ROTATION`) and leaves it, so the backend turns the poster. Out of every
  backend comes a frame as it is meant to be seen, in the width and height the library
  already stores (`video::read_meta` swaps them for a quarter turn at scan time).
- **Times are not all one clock.** GStreamer's run about two frames late on an MP4 with an
  edit list (67 ms on the spike's files); the Media Engine hands over one black frame with a
  negative time before the first. A frame with no time of its own is dropped, and a
  position shown is the player's own answer, not a frame's.
- **A missing GStreamer plugin is an error, not a crash.** On the machine this was built on,
  with no plugins, every call answered "no element `videoflip`" and went on. The web view
  aborted on the same lack (`2026-09-26-photon-video-design.md`, the spike there); the native
  window does not.
- **AVFoundation's player item wants the main thread** (`MainThreadMarker`), which eframe's
  window has. Posters do not: the image generator is made and used on any thread.

## What is built

### `video`, in photon-ui

A module of the native window, not of photon-core or photon-engine: the Tauri photon keeps its
own path until the switch-over, and photon-engine's manifest is held free of anything a
window needs (`the_engine_depends_on_no_ui_runtime`; a system's media framework is not a UI
runtime, but nothing in the engine needs one either).

- `video/mod.rs`: what every backend is - `poster(path, at) -> Result<Picture, Failure>`,
  `Player::open(path) -> Result<Player, Failure>` and the player's `play`, `pause`,
  `seek(at)`, `set_volume(v)`, `set_muted(m)`, `position()`, `duration()`, `ended()` and
  `frame() -> Option<Picture>` (the newest frame due, once). A `Picture` is RGBA, rows packed,
  turned as the video is seen, with its time. `Failure` says `Unsupported` (no decoder for
  it: the engine's `VideoFailure::Unsupported`, which skips the video for the session and
  leaves it `Pending`) or `Broken` (`Decode`, which fails it).
- `video/gst.rs` (Linux), `video/avf.rs` (macOS), `video/mf.rs` (Windows): the spike's code,
  made a module. The Linux build links GStreamer's libraries (the core, and plugins-base's
  app and video), which the web view's photon already needs at launch; its plugins are found
  when a pipeline is made, so a system without them still starts and says so where a video
  is.
- Whether the system can play a video at all is asked once, at launch, off the UI thread
  (`video::can_play`): GStreamer's MP4 demuxer and an H.264 decoder present; AVFoundation and
  Media Foundation, yes.

### Posters, made by the window

The engine's list of videos waiting for a poster (`next_video_job`, `put_video_frame`,
`video_frame_failed`, behind them `ThumbService`'s claims and `thumbs/inflight.rs`'s guard)
was built for the web view's thumbnailer and is used as it stands: the window opens a session
at launch (`video_session_start`, with `can_play`'s answer) and a thread takes jobs one at a
time, makes the poster at `min(1 s, duration / 10)` - the first frame is often black - fitted
to the preview's 1600 pixels, encodes it as a JPEG and hands it over. A video the system
cannot decode is reported `Unsupported`; one that fails or takes more than fifteen seconds,
`Decode` or `Timeout`, as the web view's did. The crash-loop guard counts a decoder that takes
the window down, as it counted the web view's process. One job at a time, as before: a
media pipeline is heavy.

### Playback, in the viewer

The viewer's video stops saying "The native window does not play videos yet." What the web
view's viewer did, the window does (`2026-09-26-photon-video-design.md`, "Viewer"):

- It **plays on opening, with its sound**, over its poster, which stays until the first frame
  is drawn. A frame is a texture of its own, written in place frame by frame
  (`TextureHandle::set`), at the video's own size, fitted as a photo is; the window asks for
  a frame when the next is due and draws nothing between.
- **Its own controls**, in the strip the web view drew (`VideoControls.svelte`,
  `createVideoPlayer`): play and pause, the time, a position bar dragged to seek (Escape
  mid-drag puts it back), the length, mute, volume, and loop. A click on the picture plays
  and pauses. Keys: Space, `L`, `M`, Shift with the left and right arrows for five seconds.
  Loop, mute and volume carry from one video to the next while photon is open, and are
  stored nowhere.
- Leaving it - a step, closing, another picture of the photo - stops it and lets its player
  go. No zoom, pan, turn or crop, as now. The slideshow passes videos over, as now.
- A video the system cannot play keeps its poster, or a placeholder, and says so: on Linux
  naming GStreamer's good and libav plugins. "Open in default app", the way out, is in its
  place and takes its press in sub-project 7, which builds it; until then the line says what
  is missing and nothing more.

### What is not built

Video for the Tauri photon (unchanged until the switch-over). The packages' dependencies -
the `.deb`'s on GStreamer's plugins, the AppImage's lack of them - which are sub-project 7's.
Hardware frames handed to the GPU without a copy: the table's 26 to 30 frames a second of
1080p through the CPU are a video's own rate, and a 4K video at 60 is the known cost (it may
drop frames). Anything the web view's video design left out.

## The pull requests

1. **The backends.** `video` with its three implementations and their tests, run on each CI
   runner over the spike's files; CI's Linux job given GStreamer's development files and
   plugins. Nothing in the window uses it yet.
2. **Posters.** The worker, the session, the fallback's words where nothing can be decoded;
   tiles of videos get their posters in the native window.
3. **Playback.** The player in the viewer, its frames, the poster under them, its failure.
4. **The controls.** The strip, the position bar, mute, volume, loop and the keys.

Each with the review every pull request of the native window has had.

## Testing

The backends are tested on the machines that have them: each runner plays the spike's files
through its own backend in CI. A test of a backend that has no decoder for a file - HEVC on
Windows, anything on this machine - asks the backend first (`can_play`, or the file's own
`Unsupported`) and passes over itself saying why; it never passes for having asked nothing.
Everything above the backends - the worker's rules, the player's state, the controls - is
tested against a player the test drives itself (a `Player` made of frames a test hands it),
in state modules and in whole frames of the application, as the viewer has been. A test that
needs real time waits for a state, as every test of the whole application does.
