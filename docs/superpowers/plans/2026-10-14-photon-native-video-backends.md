# The native window's video: the backends (sub-project 5, PR 1)

Spec: `docs/superpowers/specs/2026-10-14-photon-native-video-design.md`. Executed inline, one
review of the whole branch at the end, as every pull request of the native window has been.

**Goal.** `photon_ui::video`: a poster at a time, a player with its sound, frames as RGBA
turned as the video is seen - on each system through its own framework - and their tests,
run by CI on each runner over the spike's files. Nothing in the window uses it yet.

## Global constraints

- photon writes no video file and opens none for writing: every backend reads.
- The Linux build links GStreamer's core (`gstreamer`, `gstreamer-app`, `gstreamer-video`
  0.25); its plugins are looked for when a pipeline is made, and a missing one is a
  `Failure`, never a panic. macOS: objc2 0.6 and its 0.3 framework crates, with
  `objc2-av-foundation`'s `objc2-core-media` feature, which its default leaves out. Windows:
  `windows` 0.62, the version already in the lockfile.
- AVFoundation's player item wants the main thread (`MainThreadMarker`): `Player::open` says
  so as a `Failure` off it. The window calls it from `App::ui`, which is the main thread.
- The test binary for the backends runs every test on the main thread (`harness = false`,
  `libtest-mimic`, one thread), which macOS needs and the others do not mind. nextest runs
  it as it runs any other.
- A test that needs a decoder the system has not got says so and passes over itself - and
  only then: `video::can_play()` false, or the file's own `Failure::Unsupported`. A
  `Broken` is a failure of the test.
- No notices yet: the binding crates' licences join `THIRD-PARTY-NOTICES.md` with the
  packaging, in sub-project 7.

## Tasks

1. **`video/mod.rs`** (a state module: no egui type): `Picture { width, height, at, rgba }`,
   `Failure { Unsupported(String), Broken(String) }`, `turn(picture, quarter_turns)` (pure,
   tested on a 2x3 picture each way), `poster_time(duration_ms)` = `min(1 s, d/10)`, and
   the platform's `poster`, `can_play` and `Player`.
2. **`video/gst.rs`**: the spike's GStreamer code as `poster` and `Player` (`playbin`,
   `appsink` RGBA, `videoflip video-direction=auto`); `play`, `pause`, `seek`
   (`FLUSH|ACCURATE`), `set_volume`, `set_muted`, `position`, `duration`, `ended` (an EOS
   on the bus), `frame` (the newest sample, without waiting). `can_play`: `qtdemux` and an
   H.264 decoder in the registry.
3. **`video/avf.rs`**: the spike's AVFoundation code; the player's frames turned by the
   video track's `preferredTransform`, as the image generator turns the poster.
4. **`video/mf.rs`**: the spike's Media Foundation code; the poster turned by the reader's
   `MF_MT_VIDEO_ROTATION`, as the Media Engine turns what it plays; the engine's frame of
   no time dropped.
5. **Tests** (`tests/video/main.rs`, its own binary): posters of H.264 at 0.4 s and 2.5 s
   by colour and size; HEVC the same where the system decodes it; the turned file's poster
   and played frames 240 by 320, red over blue or blue over red; playback in order of
   colour (a frame's time within a frame's length of a second's edge is let off), position
   moving and stopping at a pause, a seek to 3.5 s white, the duration about 4 s, the end
   reached; volume and mute set without a failure. The spike's four files under
   `tests/video/` (112 KB).
6. **CI**: the Linux job installs GStreamer's development files and the good, bad and libav
   plugins.
7. **CLAUDE.md**: the module, its three backends and their tests, in the native window's
   section.

## Review focus

The things a reader of the spike's numbers might miss: a missing plugin answered as a panic
anywhere; a backend that turns twice (the poster through a framework that already turned
it); a frame handed over before the player's first (the Media Engine's negative stamp);
a player not let go of on drop (GStreamer to `Null`, the Media Engine `Shutdown`); and
`can_play` answering yes where the H.264 decoder is missing.
