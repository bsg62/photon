# Conventions, in full

The long form of `CLAUDE.md`'s Conventions, with the history behind each rule.


- **photon never writes to, moves or deletes photo files.** The one file it writes inside a
  watched folder is Picasa's own `.picasa.ini` (or `Picasa.ini`), through `picasa::set_star`
  only, to set or clear a single `star=` line; every other byte of that file is preserved.
  An INI that is a symlink or not a regular file is refused, by the reader too: the writer
  keeps every byte it reads and renames over the link, so following a planted
  `.picasa.ini -> ~/.ssh/id_ed25519` in a shared folder copied the key into it on one star.
  Export (`photon_core::export`, spec `2026-09-20-photon-export-copies-design.md`, 2026-09-20)
  writes photo files, but only *new* ones, only where the user pointed a folder picker, and
  never inside a watched root - `Engine::export_items` refuses that destination, because the
  scanner would index the copies as new photos. No watched photo is ever opened for writing,
  and every file it does write is created with `create_new` (`O_CREAT|O_EXCL`):
  `exists()`-then-write is a race, and `exists()` follows a symlink, so a dangling link in the
  destination landed a write inside a watched folder.
  This narrowed the older "never writes inside watched folders" promise on 2026-09-16 (spec
  `2026-09-16-photon-set-star-design.md`); any further write is a spec-level decision, not a
  code change. That is why a star does not survive the renaming or moving of a single file,
  though the scanner follows the photo (`photon_core::moved`, above): the star is the INI's
  line under the file's name, a star set in photon included, and carrying it would be the
  scanner writing `star=yes` for the new name with no gesture from the user. It follows a
  renamed or moved *folder*, whose INI goes with it. The writer and the reader in `picasa.rs` share one line classifier on purpose:
  a writer with its own header/key logic drifts from the reader. Faces, contacts, albums and `hidden=` flags are read
  from the same INI and never written; keywords are read from the photo and never written (the user's renames and
  removals are `tag_rules` rows applied on read, `library/tags.rs`);
  photon's own albums and edits (turns and crops) live only in `library.db` (both are by item
  id, and a renamed or moved file keeps them because the scanner re-points its row
  (`photon_core::moved`, above); a photo moved out of the library and back, or copied and the
  original deleted later, is still a purge and a new row, which loses them - a recorded
  limitation, not a bug), and so do the faces photon detects itself (`detected_faces`,
  deleted when the switch goes off) and the people the user names among them (`people`, deleted
  with them; a name is never written to an INI or a photo); Picasa's albums are read from its
  INI, as noted above, and live nowhere else. An edit never touches the photo: it is rendered on the way to the
  screen.
- **No system library dependencies beyond the system's own.** The window links what every
  desktop has - its graphics (Vulkan, Metal or Direct3D, through wgpu), its video framework
  (GStreamer's core libraries on Linux, its plugins looked for at run time; AVFoundation;
  Media Foundation, delay-loaded), and on Linux D-Bus spoken in Rust for the portal and the
  file manager. photon's C is vendored and compiled in with `cc`:
  SQLite, libwebp and libjpeg-turbo (the mozjpeg crate, for the scaled decode of a JPEG's
  preview and the encode of an edited photo's full-size render, `photon_core::turbo`). Nothing
  wrapping a C/C++ SDK: that bar is what made packaging tractable on three platforms, and it is
  why XMP and INI parsing are hand-rolled or pure-Rust. nasm is a build tool on the CI and
  release runners only, for libjpeg-turbo's x86 SIMD code and rav1d's x86-64 assembly; a build
  without it still works, as plain C and plain Rust, because rav1d's assembly is the opt-in
  `avif-asm` feature (it fails to build without nasm rather than falling back). The
  installers' workflow turns it on for every x86-64 installer and never for arm64, where rav1d 1.1.0's published assembly
  does not build; CI tests photon-core through it. A feature cannot be limited to a target, so
  that is the workflows' job, not Cargo's. A new C dependency is a spec-level decision
  (`2026-09-29-photon-turbo-jpeg-thumbnails-design.md` is the worked example). Face detection
  kept to the same bar, which rules out ONNX Runtime and OpenCV: the network runs in `tract`,
  pure Rust but for `tract-linalg`'s assembly kernels, which it compiles in with `cc` and no
  nasm, and nothing outside `face_detect` and `face_embed` names it. Both models are files in
  `crates/photon-core/models/`, embedded with `include_bytes!` - the YuNet detector and the
  SFace recogniser (38.7 MB, Apache 2.0, in `THIRD-PARTY-NOTICES.md`) - and nothing is
  downloaded. Both run on the CPU: GPU inference has no one route across three platforms, cannot
  be tested in CI, and would speed up only the first run. The workspace `Cargo.toml` builds
  `tract-linalg`, `tract-core` and `tract-data` optimised in the dev profile: unoptimised, one
  detection takes about five seconds, and the tests run the real models.
- **AVIF is decoded in `photon_core::avif`, not by `image`**, whose AVIF decoder is dav1d (C).
  `zenavif-parse` reads the container, `rav1d` (its assembly only under `avif-asm`, above)
  decodes the AV1, and `avif/av1.rs` and `turbo.rs` (libjpeg's error manager) hold the only
  `unsafe` code in photon-core. Every full decode goes through `decode::decode_image`, which sniffs the `ftyp`
  box; the uncropped thumbnail's preview goes through `decode::preview_decode`, which tries
  libjpeg-turbo's scaled decode on a JPEG first and hands everything else to
  `decode_image`'s own path. Calling `ImageReader` directly skips
  AVIF. The container's `irot`/`imir` are applied in the decoder, so an AVIF's stored
  orientation is always 1 and its EXIF orientation is ignored. `imir` axis 1 is left-to-right,
  as libavif reads it, whatever `zenavif-parse`'s doc comment says. A panic inside rav1d cannot
  unwind out of its `extern "C"` entry points, so it aborts photon rather than failing one
  thumbnail the way the thumbnail service's `catch_unwind` handles other formats (see
  `crates/photon-core/src/avif/av1.rs`'s module doc and the spec's "Limits" section);
  `thumbs/inflight.rs` keeps that from repeating on every launch.
- **Never launch the GUI to verify a change.** Verification is the test suites and
  `native-shot`'s pictures; anything needing eyes goes on `docs/smoke-checklist.md`.
- **A new test must be demonstrated to fail with its change reverted.** A compile error is not
  proof — it shows a symbol was missing, not that an assertion discriminates behaviour. Tests
  that pass with and without the change have shipped here more than once. When a change
  genuinely cannot have one - a race with no seam, a picture only a person can judge - say so in the commit
  message and why, rather than adding a test that passes either way. **A probe that passes is
  a finding, not a formality:** it has exposed a missing test (the slideshow's still-loading
  case) and a line whose comment called it load-bearing when it did nothing (an `IS NOT NULL`
  "planner hint"), and an export collision ledger that changed no outcome. Revert with an
  exact replacement; a loose `sed` that also hits a neighbouring writer fails ten tests and
  proves nothing.
  **A probe proves the rule only for the inputs the tests actually use.** Six green probes
  on the rubber band left four of its rules undefended, because each probe only exercised
  cases the existing tests happened to cover: the range-merge rule was one of them, and
  without it a narrow band selects seven photos where three were drawn. The same blind spot
  had left `publish_if_current`'s epoch guard pinned by nothing at all — the whole app suite
  passed with it removed, including the test named after it. When a probe passes, ask what
  input would make the reverted code *differ*, and write that case.
- **A large branch gets an independent read before it merges.** Every whole-branch review here
  has found a real bug no test could see; the edits branch's was the viewer reloading on any
  unrelated library change. Point the reviewer at the effect wiring and at anything a new
  feature *arms* in old code, not only at the new code.
- **Read the code before proposing a feature.** "Date search" and camera search were pitched
  as new and already existed as search haystacks; the viewer already had a rotate key.
- Comments carry the reasoning, not the mechanics. Several exist specifically to stop a future
  reader "simplifying" a load-bearing line; a wrong justification is treated as a defect.
- Design docs live in `docs/superpowers/specs/`, implementation plans in
  `docs/superpowers/plans/`.

