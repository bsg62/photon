# The native window's video: posters (sub-project 5, PR 2)

Spec: `docs/superpowers/specs/2026-10-14-photon-native-video-design.md`, "Posters, made by the
window". Executed inline, one review of the whole branch at the end.

**Goal.** A video's tile shows its poster in the native window: a thread of the window takes
the engine's video jobs one at a time and makes each poster through `video::poster`.

## Global constraints

- The engine's job list is used as it stands: `video_session_start(supported)`,
  `next_video_job`, `put_video_frame` (a JPEG), `video_frame_failed(VideoFailure)`. Nothing
  in photon-core or photon-engine changes.
- What a failure is reported as is `video-thumbnailer.svelte.ts`'s rule: the system's lack
  and the window closing mid-job are `Unsupported` (the video waits for a session that can);
  the file's failure is `Decode`; a poster that has not come in fifteen seconds is `Timeout`;
  a poster made but not stored is `Unsupported`.
- The poster is taken at `video::poster_time(duration_ms)` - the library's length, from the
  file's `mvhd` - and fitted within 1600 pixels before it is encoded.
- No real decoder in the tests of the worker: a stand-in `Posters` answers by file name.

## Tasks

1. **`posters.rs`** (a state module): the `Posters` trait (`can_play`, `poster`, `patience`)
   and `System`; `PosterWorker::start(engine, posters)` and `finish(patience)`; `answer`, the
   rule above, with its unit tests; `jpeg`, the fit and the encode, with theirs.
2. **`App`**: the worker started at the first frame (so a test's `with_posters` comes
   first), finished before the engine at `close` - a job left unanswered is counted at the
   next launch as a window that died opening the video.
3. **`tests/posters.rs`**: a poster made and stored as the tile's thumbnail, asked of the file
   at its poster time; the system's lack leaving a video waiting while a broken and a slow
   one are failed; a system that plays nothing asked for nothing; a window closed mid-poster
   leaving its video waiting with its claim answered.
4. **CLAUDE.md**: the worker beside the backends' paragraph.

## Ruling made while planning

"The fallback's words where nothing can be decoded" (the spec's list of pull requests) are
the viewer's - the web view says them there - and go with the viewer's other words for a
video, in PR 3.

## Review focus

A poster worker that outlives the window or keeps it from closing; a claim left unanswered
on any path (each one is a death at the next launch); a failure reported as the wrong kind;
the session told it can play where it cannot (every video tile then waits for ever); a
decoder that hangs or panics taking the worker with it.
