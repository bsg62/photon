# libjpeg-turbo for JPEG thumbnails

2026-09-29. Approved in conversation the same day; implemented on `feat/turbo-jpeg-thumbnails` (PR #133).

## What it is

photon-core decodes the JPEGs it thumbnails with libjpeg-turbo, through the `mozjpeg` crate. The
decode happens at a reduced size (a scaled DCT), just large enough for the 1600 px preview.
Anything the fast path does not take on, or gives up on, is decoded by zune through `image`,
exactly as it is today. Only uncropped thumbnails change. Every full-size decode stays where it
is.

This reverses a stated rule. The README, the website, CLAUDE.md and several specs say photon
has no native dependencies. That was never literally true: SQLite and libwebp are C, vendored
and compiled into photon with `cc`. After this change the rule says what it has meant in
practice: **no system libraries beyond the system's web view**. The C that photon uses is
vendored and compiled into the binary; the only thing it needs installed on the user's machine
is the web view Tauri already requires (webkit2gtk and libsoup on Linux). The rule's purpose,
packaging that stays tractable on three platforms, is unaffected: the `ldd` check in
`release.yml` extracts the binary from the `.deb`, fails on any library "not found", and
expects the `.deb` to depend on webkit2gtk and libsoup. libjpeg-turbo adds no library to that
list.

## Why

Since the performance audit, the JPEG decode is most of a thumbnail's cost: about 110 of
roughly 175 ms for a 24 MP photo. zune-jpeg (0.5.15) already uses NEON and AVX2, but it has no
scaled decode. It always reconstructs every pixel, and `fit_within` then throws away 95% of
them.

`jpeg-decoder`'s scaled decode was measured and rejected on 2026-09-29. Its entropy decoder is
slower than zune's, and a JPEG decode is dominated by entropy decoding. libjpeg-turbo's entropy
decoder is as fast as zune's, and it scales by any n/8, not only by powers of two. So a 24 MP
photo can decode at 3/8 (2250x1500) where `jpeg-decoder` had to use 1/2.

### Measurements

These were taken on 2026-09-29 on Apple Silicon, with release builds and one thread. The fixture
is the noisy-gradient q90 JPEG from `benches/render.rs`. The time is the decode plus
`fit_within` to 1600 px, the median of 9 runs.

| Photo | zune (today) | libjpeg-turbo, NEON | libjpeg-turbo, plain C | Scale |
|---|---|---|---|---|
| 12 MP (4032x3024) | 69.5 ms | 52.6 ms (−24%) | 62.0 ms | 4/8 |
| 24 MP (6000x4000) | 123.3 ms | 93.2 ms (−24%) | 95.3 ms | 3/8 |
| 45 MP (8256x5504) | 231.8 ms | 162.6 ms (−30%) | 172.4 ms | 2/8 |

- **Full-resolution decode.** At 24 MP, libjpeg-turbo takes 95.4 ms with NEON and 142.0 ms as
  plain C, against zune's 110.0 ms. Without SIMD it is slower than zune for a full decode.
  That is one reason the full-size paths stay on zune (see Decisions).
- **Same picture.** The fast path's preview and zune's are the same picture to
  `similar::same_picture`: a mean absolute difference of 0.02-0.03 on its 32x32 reduction.
  Genuine copies measure up to 2.3, and the threshold is 4.0. Look-alike grouping cannot tell
  a thumbnail made by one decoder from one made by the other.
- **What a whole thumbnail gains.** The preview's WebP encode and the grid shrink come on
  top, so a whole 24 MP thumbnail goes from about 175 to about 145 ms per worker.
- **What was not measured:**
  - Real photos. The fixture is noise-heavy, and noise is the entropy decoding that scaling
    cannot skip. A real photo is likely to gain more, but that is unverified.
  - x86. Building the implementation includes measuring it there (see Build).

## Decisions

- **Scope: uncropped thumbnails only.** Only the scaled decode inside `decode_oriented`
  changes, which is the path `ThumbCache::render` takes when an edit has no crop.
  `decode_image` is unchanged, and so is every caller of it:
  - a crop's thumbnail (full-resolution by design: `a_crop_is_taken_at_full_size_and_only_then_shrunk`);
  - `/image/<id>`;
  - export;
  - the clipboard;
  - `render_picture`.

  The reasons:
  - That is where the gain is.
  - Only the thumbnail service has the in-flight crash guard (`thumbs/inflight.rs`). A crash
    inside libjpeg while the user views or exports a photo would take photon down with nothing
    recorded.
  - At full resolution, without SIMD, libjpeg-turbo is slower than zune.
- **Crate: `mozjpeg` 0.10 (`mozjpeg-sys` 2.2), default features.**
  - It builds with `cc`, plus nasm for x86 SIMD. The default features are kept for
    `nasm_simd`.
  - It exposes a hook for a caller's own error manager (`DecompressBuilder::with_err`), which
    the warning policy below needs.
  - `mozjpeg` 0.10.13 depends on `mozjpeg-sys` with `unwinding` unconditionally, so the
    default features do not control it. `unwinding` only adds `-fexceptions` where the C
    compiler supports it; MSVC ignores it, and unwinding through C frames works there anyway
    (CI proved it on Windows). What would break the design is a `panic = "abort"` profile:
    every hand-back is an unwind out of libjpeg, so each would abort photon. `turbo.rs`
    refuses to compile under it (`compile_error!` on `cfg(panic = "abort")`).
- **The alternative is the `turbojpeg` crate.** That is upstream libjpeg-turbo 3.x, whose API
  reports errors and warnings as return codes, which would need no `unsafe` in photon. Its
  vendored build needs cmake on every machine that builds photon, and it was not measured.
- **Damaged data falls back to zune.** When libjpeg-turbo meets damaged data (a truncated file,
  corrupt entropy data, stray bytes between markers), it issues a warning and carries on,
  filling what it could not read with grey. The `mozjpeg` crate discards those warnings.
  photon's own error manager turns a warning into an unwind, which ends the fast path, and
  zune decodes the file as it does today. So a damaged file keeps today's behaviour exactly:
  the thumbnail zune makes, or `Failed` through `Error::Image`, never a grey-filled picture
  that nothing reported.
- **x86 builds get SIMD: nasm on every CI and release runner.** Without nasm, mozjpeg-sys
  *silently* builds plain C on x86. The workflows therefore fail when nasm is missing, rather
  than trusting it to be there. A local build without nasm still works, just without SIMD on
  x86. Arm64 builds need nothing new: their NEON code compiles with `cc`.

## The decoder: `photon_core::turbo`

A new module `crates/photon-core/src/turbo.rs` has one entry point:

```rust
/// A JPEG decoded straight at the smallest n/8 scale that still covers `target` on both
/// axes, or `None` when libjpeg-turbo should not or could not decode it, in which case the
/// caller decodes it as before.
pub(crate) fn scaled_decode(bytes: &[u8], target: (u32, u32)) -> Option<DynamicImage>
```

**What it takes on.** It returns `None` before decoding anything unless every one of these
holds:
- The header parses.
- The JPEG colour space is YCbCr with three components. Greyscale, CMYK and YCCK go to zune,
  which today returns a greyscale photo as `L8` and handles Adobe's inverted CMYK. Handing
  those over keeps every thumbnail's pixel format what it is today.
- The file is not arithmetic-coded. mozjpeg-sys builds without `arith_dec`, so those files go
  to zune, whose answer today stands.
- The header's dimensions fit the limit `image` applies: width x height x 4 bytes within
  512 MiB. A larger file goes to zune, which refuses it with the same error as today. This
  keeps the per-decode bound that `MAX_WORKERS` is reasoned against (`decode.rs`, the
  `decode_oriented` doc).

**Scale.**
- **The target** is what the thumbnail must end up as: `fitted(width, height, max_edge)`, the
  size `image`'s resize gives and that every cached thumbnail already has. It is computed from
  the photo's *own* dimensions, not from the scaled decode's.
- **The scale** is the smallest n in 1..=8 for which libjpeg's output
  (`ceil(width * n / 8)` by `ceil(height * n / 8)`) is at least the target on both axes.
- **The shrink to size.** The scaled picture is then resized to exactly the target, with the
  same bilinear filter as `fit_within`. That is a new helper in `decode.rs`,
  `resize_to(img, (w, h))`, private to `decode.rs`, which shares its resize with
  `fit_within_by` through a private `resize_by`, so there is one resize.
  `fit_within` itself cannot be reused on the scaled picture: it computes `fitted` from the
  picture's own size, and ceiling rounding at n/8 can land a pixel away from what the full
  photo gives.
- **Photos already small enough.** If the photo is no larger than `max_edge`, there is nothing
  to scale, and `decode_oriented` does not call `scaled_decode`. A small photo stays on zune,
  so it is decoded exactly as before.

**Errors, and the `unsafe` code.**
- photon supplies its own `jpeg_error_mgr` through `with_err`. It starts from
  `jpeg_std_error`, then replaces two callbacks:
  - `error_exit` formats nothing and unwinds with `std::panic::resume_unwind`, which, unlike
    `panic!`, runs no panic hook. A decoder refusing a photo is not a bug and must not log as
    one.
  - `emit_message` does the same when `msg_level == -1`, which is libjpeg's warning level, and
    ignores trace messages (levels 0 and up).
- The whole decompress lifecycle, including the drop that calls `jpeg_destroy_decompress`,
  runs inside one `catch_unwind`, and any unwind becomes `None`. So nothing from libjpeg ever
  reaches `catch_unwind` in `service.rs`, and a file the fast path gives up on costs one wasted
  partial decode.
- **The cost of a late warning.** A warning late in the file, such as padding before the end
  marker, which some cameras write, costs nearly a whole libjpeg decode before zune starts.
  Such a file is slower than today. The smoke import (see Testing) counts how many real camera
  files take the fall-back. If it is more than a few percent of a camera's files, that warning
  is revisited before release, not waved through.
- The only `unsafe` in the module is the error manager's construction (`mem::zeroed` and
  `jpeg_std_error`). The callbacks are `extern "C-unwind"` functions, as the crate requires.
- CLAUDE.md's sentence that `avif/av1.rs` "holds the only `unsafe` code in photon-core" is
  updated to name this module as the second.

**Crashes.** A crash inside libjpeg (a segfault, an allocation failure) is the case
`thumbs/inflight.rs` already exists for. `process` writes an in-flight marker around every image
decode, whatever the format. After two deaths on the same photo it is marked Failed, with
`CRASH_MESSAGE`, instead of crash-looping at every launch. Nothing new is needed.

## Wiring

`decode_oriented` (`decode.rs`) becomes:
1. Open the file and sniff it as today.
2. **Take the fast path** if all of these hold: the photo is a JPEG (`jpeg::is_jpeg`), its long
   edge exceeds `max_edge`, and `scaled_decode` returns `Some`. The result is the picture at
   exactly `fitted(...)`.
3. **Otherwise** decode from the bytes already read and fit them, as before:
   `fit_within(decode_from(Cursor::new(bytes), path)?, max_edge)`.
4. Apply the orientation, as today.

The bytes are read once. The fast path needs the whole file in memory, which `image`'s JPEG
decoder already requires (`JpegDecoder::new` starts with `read_to_end`). A fall-back hands those
bytes to `image` from a `Cursor` rather than reading the file again.

So that a test can see which decoder ran, the body lives in a `pub(crate)` function that
reports it:

```rust
pub(crate) enum PreviewDecoder { Turbo, Image }

pub(crate) fn preview_decode(
    path: &Path,
    max_edge: u32,
) -> Result<(DynamicImage, PreviewDecoder)>
```

`decode_oriented` calls it and drops the second value. Nothing outside the tests reads it.
The fall-backs are logged, at debug level, which is how the smoke import counts them (see
Testing). `scaled_decode` emits one of two events when it returns `None`: "libjpeg-turbo warned
or failed" when libjpeg's warning or error unwound, and "libjpeg-turbo does not take this JPEG
on" for a deliberate hand-back (colour space, decode limit, no covering scale). Neither names
the file, so `preview_decode` enters a `preview` span carrying the path around the call, and
both events carry it.

## Build

- **Dependency.** `crates/photon-core/Cargo.toml` gets `mozjpeg = "0.10.13"` with default
  features: `unwinding`, `nasm_simd` and `parallel`, where `parallel` only parallelises the C
  build. A comment beside it says why the defaults stay (`nasm_simd`) and what would break the
  design (`panic = "abort"`).
- **nasm in CI.** In `.github/workflows/ci.yml`, the `rust` job installs nasm on all three
  runners with the runner's own package manager: `apt-get` on Linux, Homebrew on macOS,
  Chocolatey on Windows (adding `C:\Program Files\NASM` to the PATH). It then runs `nasm -v`
  as its own step, so a missing nasm fails the job by name instead of silently producing a
  plain-C build. There is no third-party action: nasm's official macOS binaries are x86_64,
  and Homebrew's is native to the arm64 runners.
- **nasm in releases.** `.github/workflows/release.yml` does the same in the Linux, Windows
  and macOS jobs.
  - The macOS matrix cross-compiles `x86_64-apple-darwin` on an arm64 runner. nasm assembles
    `macho64` there regardless of the host, and mozjpeg-sys picks the format from the target.
  - The first release build after the change confirms both `.dmg`s link. The publish job's
    check for two `.dmg`s already fails otherwise.
- **x86 measurement before merging.** The PR runs `cargo bench -p photon-core --bench render`
  on an x86 GitHub runner (Linux or Windows), once with nasm and once without. The numbers go
  in the PR description.
  - This is the check that nasm actually bought the SIMD path, which no test can see from
    inside the binary.
  - If x86 with SIMD does not beat zune's decode-plus-fit on that runner, the change does not
    merge as designed. The fast path becomes arm64-only, through a target-specific dependency,
    and this spec is amended.
  - **Measured** (the `thumbnail_24mp` group, `preview_zune` then `preview_turbo`, each on its
    own runner, so compare the ratios rather than the times across rows):

    | x86 runner | zune | libjpeg-turbo |
    |---|---|---|
    | Linux, with nasm | 207.32 ms | 155.61 ms |
    | Windows, with nasm | 181.24 ms | 110.40 ms |
    | Linux, without nasm | 182.28 ms | 135.36 ms |

    Both legs with nasm beat zune. The Windows no-nasm leg was not measurable: the Windows
    runner ships nasm at `C:\Strawberry\c\bin`, so it could not be taken away, and that leg
    was waived.
- **Security fixes (checked 2026-09-29).** The vendored tree is mozjpeg 4.1.5
  (`mozilla/mozjpeg@c2bc351`), whose libjpeg-turbo base is 2.1.x (`ChangeLog.md` opens at
  2.1.6; its five entries are the first five of upstream's 2.1.x branch, "2.1.6 ESR"). Read:
  upstream `ChangeLog.md` from 2.1.91 to 3.2.1, the 2.1.x branch's 2.1.6 ESR section, and
  upstream's security advisories (none published). (A reference like "3.0.4[6]" is
  "version[item]": the item's number in that version's section of upstream's `ChangeLog.md`.)
  None of upstream's decoder security fixes
  after 2.1.5.1 is both missing from the vendored tree and reachable from 8-bit lossy
  decompression as photon drives it (`Decompress::with_err(..).from_mem`, `scale`, `rgb`,
  `read_scanlines`, no saved markers). Each fix is one of:
  - **Present in the vendored tree.** `2e1b8a46` (`jpeg_crop_scanline`'s width with scaling
    and 4x2/2x4 sampling, 3.0.0[4]): `jdapistd.c`, `jpeg_crop_scanline` divides by
    `max_h_samp_factor * _min_DCT_scaled_size`. `42ce199c` (two-pass quantisation with RGB565,
    3.0.0[3]): `jdmaster.c`, `master_selection`, and `jquant2.c`, `jinit_2pass_quantizer`, both
    test `JCS_RGB565`. The smoothing fixes `eadd2436` and `a9d87361` (3.0.1[2], not security
    fixes) are in `jdcoefct.c`'s `decompress_smooth_data`. `jdcoefct.c`, `jdmaster.c` and
    `jquant2.c` are byte-identical to the 2.1.x branch's.
  - **Absent, and only in libjpeg calls photon never makes.** `9046ae19` (quadratic time
    saving many markers, 3.0.4[1]: `jdmarker.c`'s `save_marker`, which only
    `jpeg_save_markers` installs, and the `mozjpeg` crate calls that only for `with_markers`).
    `61709c85` (crop bounds overflow, 3.0.4[6]), `79dd838c` (crop with raw output, 3.2.0[3])
    and `2646fa33` (merged upsampler overrun when cropping without SIMD, 3.2.1[7]), all in
    `jpeg_crop_scanline`. `9e17b981` (use after free, 3.1.2[3]) and `7b5fee3f` (skip counts,
    3.2.1[9]), both under `jpeg_skip_scanlines`. The `mozjpeg` crate calls neither
    `jpeg_crop_scanline` nor `jpeg_skip_scanlines`. **So `turbo.rs` never calls
    `with_markers`, and never reaches libjpeg's crop or skip through `mozjpeg::ffi`**: each is
    a known bug left unfixed in this tree.
  - **Absent, and needing 3.x's several precisions.** `3c17063e` (duplicate SOF, 3.0.4[2]) also
    needs a source manager that returns `FALSE` at end of data, where the crate's inserts a
    fake EOI; 2.1's 8-bit build refuses the 12-bit SOF (`jdinput.c`, `initial_setup`), and the
    2.1.x branch did not take it. `e0e18dea` (3.1.1[1]) guards 3.x's per-precision methods.
  - **Off the path by kind.** Lossless (CVE-2023-2804, 3.0.0[2]); 12-bit with quantisation and
    RGB565 (3.2.1[8]); the TurboJPEG C and Java APIs (3.0.0[5], 3.0.2[1], 3.0.4[7-9],
    3.1.3[1-3], 3.1.4[1,5,6,8], 3.2.1[6,10]); jpegtran, djpeg, TJBench and the image loaders
    (3.0.3[4], 3.1.2[1], 3.1.4[4], 3.2.0[2,4], 3.2.1[4-5], CVE-2026-75466); the compressor
    (3.0.0[7], 3.0.1[3], 3.1.4[3,7]).
  - **Without a ChangeLog entry**, on the 2.1.x branch: `68321702` (`jmemmgr.c` sizes in
    `size_t`; a UBSan report, 64-bit `long` on Linux and macOS, and on Windows the 512 MiB
    header limit keeps a coefficient array far below 2^31 bytes) and `3e6e5673` (`jerror.c`
    zeroes the error manager, which photon's `error_mgr` already does).

## Documentation and notices

- **README, "File formats".** The last sentence becomes:

  > Camera RAW files and HEIC are not read: decoding them means shipping a large C library,
  > and photon keeps its C to a few small, vendored ones (SQLite, libwebp, libjpeg-turbo),
  > compiled in, so it needs no system libraries beyond the system's web view.

- **README, JPEG sentence.** A sentence says JPEG thumbnails are decoded with libjpeg-turbo.
- **Website.** `site/index.html`'s "What it doesn't do" item is reworded the same way.
- **CLAUDE.md, Conventions.**
  - "No native library dependencies" becomes "No system library dependencies beyond the web
    view". The paragraph
    names the vendored C (SQLite, libwebp, libjpeg-turbo, each compiled with `cc`), says that
    nasm is a build tool on the CI and release runners only, and keeps "nothing wrapping a
    C/C++ SDK" as the bar for anything new.
  - The `unsafe` sentence changes as noted above.
  - "Every full decode goes through `decode::decode_image`" gains a clause: the uncropped
    thumbnail's scaled decode tries `turbo::scaled_decode` first.
- **`THIRD-PARTY-NOTICES.md`.** It gains an entry for mozjpeg/libjpeg-turbo in the file's usual
  shape: what depends on it and why, then the licence texts in full. That means the IJG
  licence (the "based in part on the work of the Independent JPEG Group" clause is already in
  the jpeg-encoder entry), the Modified BSD licence, and the zlib licence that covers the SIMD
  code.

## Testing

**Decoder (`turbo.rs`).** The fixtures are thin noisy strips, not multi-megapixel squares, per
CLAUDE.md's note on fixture cost. For example, 3300x40 decodes at 4/8 for a 1600 px target.
- **Scale.** The chosen n is the smallest that covers the target, checked against a table of
  camera sizes and edge cases. This includes a width at which n/8's ceiling lands a pixel off
  `fitted`, and the output is exactly `fitted(...)`.
- **Same picture.** A noisy strip decoded by the fast path and by zune plus `fit_within` has
  the same dimensions and colour type, and is the same picture within
  `SAME_PICTURE_MAX_DIFFERENCE / 4.0`. That is the tolerance `decode.rs` already holds its
  resampler to, for the same reason: caches mix old and new thumbnails.
- **Cases that return `None`:**
  - a greyscale JPEG;
  - a CMYK JPEG;
  - a truncated JPEG (a warning);
  - a JPEG with stray bytes between segments (a warning), built by hand from a comment
    segment and two extra bytes;
  - corrupt entropy data (a warning, or a fatal error);
  - bytes that are not a JPEG (a fatal error);
  - a header over the size limit, which `scaled_decode_within` takes as an argument so that a
    small file can show the limit is applied (a header claiming 60000x60000 over a small
    file's data would be handed back by libjpeg's end-of-file warning either way).
- **No panic hook.** A `None` from a fatal error leaves a panic hook installed for the test
  uncalled.

**Wiring (`decode.rs`).**
- `preview_decode` reports `Turbo` for a plain JPEG larger than `max_edge`, and `Image` for
  one no larger, and for PNG, AVIF and each of the fall-back cases above.
  - This is the test that fails when the fast path is disconnected.
  - It runs on all three CI OSes, which also proves the unwind through libjpeg works under
    MSVC, where `-fexceptions` is not a `cl.exe` flag.
- A damaged JPEG gives the same result through `decode_oriented` as through `decode_image`
  plus `fit_within`: the same image, or the same `Error::Image`. That keeps it Failed rather
  than retried, per `is_source_defect` in `service.rs`.
- The existing tests stand unchanged:
  - `decode.rs`: sizes against `image`, a corrupt JPEG giving `Error::Image`, routing by
    content;
  - `thumbs/cache.rs`: sizes, orientation, the crop test;
  - `similar.rs`: grouping through `ThumbCache::generate`.

  Any of them that moves is a finding to explain, not a threshold to loosen.

**Benchmark.** `benches/render.rs`'s `thumbnail_24mp` group gains `preview_zune` (zune's
decode then `fit_within`) and `preview_turbo` (`decode_oriented` at orientation 1, which takes
the fast path). `preview_decode` is `pub(crate)`, out of a bench's reach, so the two paths are
compared through public functions. The CI gate in CLAUDE.md compiles only the grid bench, so
the PR also runs `cargo bench -p photon-core --bench render --no-run` once.

**Probes, per CLAUDE.md.** Each new assertion is shown to fail with its rule removed:
- the warning callback made to return instead of unwinding (the truncated file then decodes
  grey, and the equality test fails);
- the colour-space check removed (greyscale comes back `Rgb8`);
- the target computed from the scaled size (the off-by-one width fails);
- `decode_oriented` routed straight to `decode_image` (`preview_decode` reports `Image`).

**Smoke checklist.** A new item: import a folder of real camera JPEGs and check that the
thumbnails and previews look as before, with the times of a before and an after import noted.
The "libjpeg-turbo warned or failed" lines, each carrying the photo's path through
`preview_decode`'s span, are counted over the folder; greyscale and CMYK files log "does not
take this JPEG on" instead, which is expected and not counted. This is where "real photos gain more" is confirmed or corrected, and where
the late-warning fall-back is measured.

## Rollout

- **No schema change, no cache change.** Thumbnail keys do not change, so no cached thumbnail
  is rebuilt. New thumbnails come from the fast path and sit beside old ones that
  `same_picture` cannot tell apart.
- **Release.** A minor release: the README's promise about native code changes, and the
  release notes' "Upgrading" section says so in a sentence. The notes also say that nothing
  about the user's files or library changes.

## Not in this design

- **Full-size decodes** through libjpeg-turbo: `/image`, export, the clipboard, a crop's
  thumbnail. Revisit only with the in-flight guard extended to those paths, and with x86 SIMD
  numbers showing a full decode beats zune.
- **rav1d's assembly for AVIF.** It was measured at −41% on arm64 with bit-identical output,
  but crates.io's rav1d 1.1.0 omits `src/arm/asm-offsets.h`, so its arm64 assembly does not
  build from the published package. It waits for an upstream release.
- **Pre-existing gaps** found while writing this, each its own change:
  - `THIRD-PARTY-NOTICES.md` does not carry libwebp's or SQLite's notices. libwebp's BSD
    licence asks for its notice in binary distributions.
  - Two specs (`2026-09-21-photon-avif-design.md` and `2026-09-26-photon-video-design.md`) say
    `xtask metadata` enforces the notices file. It does not check it.
- **Decoding previews in parallel within one photo.** libjpeg-turbo is single-threaded per
  decode, and the thumbnail pool supplies the parallelism.
