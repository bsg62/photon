# libjpeg-turbo for JPEG thumbnails: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decode the preview of an uncropped JPEG thumbnail with libjpeg-turbo at the smallest
n/8 scale that covers it, handing every file libjpeg does not take on, warns about or fails on
back to zune, which decodes it exactly as it does today.

**Architecture:**
- **`photon_core::turbo`** (new) wraps the `mozjpeg` crate behind one function,
  `scaled_decode(bytes, target) -> Option<DynamicImage>`. It has its own libjpeg error manager,
  which unwinds on an error *or a warning*, all inside a `catch_unwind`.
- **`decode.rs`** gets `preview_decode`. It reads a JPEG whole, tries `scaled_decode` against
  the photo's own `fitted` size, resizes the result to exactly that size, and otherwise falls
  back to `decode_image` plus `fit_within` over the same bytes. `decode_oriented` calls it.
- **Everything else is unchanged:** every full-size decode, and every non-JPEG.
- **Build.** CI and release install nasm, so x86 builds get libjpeg-turbo's SIMD code.

**Tech stack:**
- Rust 2024 (MSRV 1.93).
- `mozjpeg` 0.10.13 and `mozjpeg-sys` 2.2.3, which vendor mozjpeg 4.1.5, C compiled with `cc`,
  plus nasm on x86.
- `image` 0.25 (zune-jpeg) as the fallback.
- GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-29-photon-turbo-jpeg-thumbnails-design.md`. Read it
first: this plan argues from it.

## Global constraints

- **Scope.** Only `decode_oriented`'s decode changes. `decode_image`, `render_picture`,
  `render_full`, `clipboard_picture`, export, `/image/<id>` and a crop's thumbnail are not
  touched.
- **Fall back on any complaint.** Any libjpeg warning or error falls back to zune. A damaged file
  must end exactly as today: zune's image, or `Error::Image`.
- **Only three-component YCbCr JPEGs take the fast path.** Greyscale, CMYK, YCCK and
  arithmetic-coded files go to zune.
- **Decode limit.** Header width × height × 4 must be at most 512 MiB (`image`'s default
  `max_alloc`). A larger file goes to zune.
- **Size.** A fast-path preview is exactly `fitted(width, height, max_edge)` of the photo's own
  dimensions.
- **Features.** `mozjpeg` keeps its default features. `unwinding` must stay on, because without
  it a libjpeg error aborts photon.
- **`unsafe`.** The only new `unsafe` code is the error manager's construction in `turbo.rs`.
- **nasm.** It is installed, and `nasm -v` checked, on every CI `rust` runner and every release
  build job.
- **The repo's rules still apply.** CLAUDE.md governs: a new test must be shown to fail with its
  rule reverted, comments carry the reasoning, and there is no `sed` on source files for edits.
- **Branch.** Work on `feat/turbo-jpeg-thumbnails`, created from `spec/turbo-jpeg-thumbnails`
  (which holds the spec and this plan).

**The Rust gate** (CLAUDE.md), run before *every* commit in this plan, from the repo root:

```bash
cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```

---

### Task 0: Check the vendored libjpeg-turbo for missing decoder security fixes

This gates the whole design (spec, "Build"). It changes no code.

**Files:**
- Modify: `docs/superpowers/specs/2026-09-29-photon-turbo-jpeg-thumbnails-design.md` (the
  "Security fixes" bullet in "Build")

**Facts already established (2026-09-29):**
- `mozjpeg-sys` 2.2.3's `vendor` is a submodule of `mozilla/mozjpeg` at commit
  `c2bc3511d547870c4f67bbafda5fba794ba71f7f` ("Allow disabling fopen in SIMD", 2025-01-21).
- Its `CMakeLists.txt` says `set(VERSION 4.1.5)`.
- Its `ChangeLog.md` opens at libjpeg-turbo `2.1.6`, then `2.1.5.1`, then `2.1.5`.

- [ ] **Step 1: Create the branch**

```bash
git switch spec/turbo-jpeg-thumbnails
git switch -c feat/turbo-jpeg-thumbnails
```

- [ ] **Step 2: Fetch the two changelogs and the advisories**

```bash
S=$(mktemp -d)
gh api "repos/mozilla/mozjpeg/contents/ChangeLog.md?ref=c2bc3511d547870c4f67bbafda5fba794ba71f7f" --jq .content | base64 -d > "$S/vendored-ChangeLog.md"
gh api "repos/libjpeg-turbo/libjpeg-turbo/contents/ChangeLog.md" --jq .content | base64 -d > "$S/upstream-ChangeLog.md"
gh api "repos/libjpeg-turbo/libjpeg-turbo/security-advisories" --jq '.[] | [.ghsa_id, .cve_id, .severity, .summary] | @tsv' > "$S/advisories.tsv"
echo "$S"
```

- [ ] **Step 3: List the upstream fixes the vendored tree may lack**

Read every upstream `ChangeLog.md` section newer than `2.1.5.1`: the `2.1.6` section if it
exists, then `3.0.0` through the newest. Also read `advisories.tsv`. Keep an entry only if it
fixes a buffer overrun, overread, use of uninitialised memory, crash, hang, or anything
OSS-Fuzz found, **and** the code it touches is on photon's path.

**Photon's path** is 8-bit lossy decompression through the libjpeg API:
- the marker reader (`jdmarker.c`);
- the Huffman decoders (`jdhuff.c`, `jdphuff.c`);
- coefficient handling (`jdcoefct.c`);
- upsampling and colour conversion (`jdsample.c`, `jdmerge.c`, `jdcolor.c`, `jdcol565.c`);
- the scaled inverse DCTs (`jidctred.c`, `jidctint.c`);
- `jdapimin.c` and `jdapistd.c`;
- the SIMD decompression routines.

**Not on the path:**
- lossless JPEG;
- 12- and 16-bit precision (introduced in 3.0, so absent from the 2.1 base anyway);
- the TurboJPEG API (`turbojpeg.c`), which mozjpeg-sys builds without `turbojpeg_api`;
- the command-line tools;
- the compressor.

For each kept entry, find its commit (the ChangeLog cites issue numbers). Then check whether
the vendored tree has it:

```bash
gh api "repos/mozilla/mozjpeg/contents/<file>?ref=c2bc3511d547870c4f67bbafda5fba794ba71f7f" --jq .content | base64 -d | less
```

Compare that against the fix's diff, which you get from
`gh api repos/libjpeg-turbo/libjpeg-turbo/commits/<sha> --jq '.files[] | .filename + "\n" + .patch'`.

- [ ] **Step 4: Decide**

- **If any kept fix is absent from the vendored tree: STOP.** Report the list to the user. The
  spec then switches to the `turbojpeg` crate, which is a design change for the user to
  approve, not something to improvise.
- **Otherwise:** replace the spec's "Security fixes" bullet in "Build" (the one that starts
  "**Security fixes.** mozjpeg is a fork of libjpeg-turbo.") with a dated record, in this
  shape:

```markdown
- **Security fixes (checked 2026-MM-DD).** The vendored tree is mozjpeg 4.1.5
  (`mozilla/mozjpeg@c2bc351`), whose libjpeg-turbo base is 2.1.x (`ChangeLog.md` opens at
  2.1.6). Upstream's decoder security fixes from after that base, and the ones that apply to
  8-bit lossy decompression, are: <list, each with its upstream commit and "present in the
  vendored tree" and where>. <Or: "None of upstream's fixes after 2.1.5.1 touch 8-bit lossy
  decompression: <the fixes read and why each is off the path>.">
```

Every angle-bracketed part is filled in from Steps 2 and 3. None is left in the committed text.

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/specs/2026-09-29-photon-turbo-jpeg-thumbnails-design.md
git commit -m "docs(spec): the vendored libjpeg-turbo's security fixes, checked

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

(A docs-only commit. The Rust gate has nothing to check, but running it costs nothing.)

---

### Task 1: `photon_core::turbo`: the scaled decode

**Files:**
- Modify: `crates/photon-core/Cargo.toml` (`[dependencies]`)
- Modify: `crates/photon-core/src/lib.rs` (module list)
- Modify: `crates/photon-core/src/testutil.rs` (two fixture helpers)
- Create: `crates/photon-core/src/turbo.rs`

**Interfaces:**
- Produces, used by Task 2:
  - `pub(crate) fn scaled_decode(bytes: &[u8], target: (u32, u32)) -> Option<image::DynamicImage>`
    in `crate::turbo`. It returns an `ImageRgb8` of at least `target` on both axes, or `None`.
- Produces (test helpers, `crate::testutil`):
  - `pub fn noisy_rgb(w: u32, h: u32) -> DynamicImage`
  - `pub fn noisy_jpeg(w: u32, h: u32) -> Vec<u8>`

- [ ] **Step 1: Add the dependencies**

In `crates/photon-core/Cargo.toml`, directly after the `memchr = "2.8.3"` line, add:

```toml
# libjpeg-turbo, as the mozjpeg fork vendors it, for the scaled decode of a JPEG's preview
# (`turbo.rs`). Default features on purpose: `unwinding` compiles libjpeg with `-fexceptions`,
# which is what lets `turbo`'s error manager unwind out through it (without it a libjpeg error
# aborts photon), and `nasm_simd` builds the x86 SIMD code wherever nasm is installed, which
# the CI and release workflows make sure of.
mozjpeg = "0.10.13"
# The C declarations `turbo`'s error manager is built from; `mozjpeg` does not re-export them.
mozjpeg-sys = "2.2.3"
```

Run: `cargo build -p photon-core`
Expected: it compiles, including mozjpeg-sys's C, and `Cargo.lock` gains `mozjpeg`,
`mozjpeg-sys`, `nasm-rs` and `rgb`.

- [ ] **Step 2: Add the fixture helpers**

In `crates/photon-core/src/testutil.rs`, directly after `pub fn jpeg_bytes` (which ends
`encode(&solid(w, h), ImageFormat::Jpeg)` / `}`), add:

```rust
/// A gradient under per-pixel noise, the same every time: a photo stand-in whose JPEG blocks
/// all carry detail, where a solid colour's (`jpeg_bytes`) are empty and decode in no time.
pub fn noisy_rgb(w: u32, h: u32) -> DynamicImage {
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let noise = (seed & 0x3F) as u32;
        Rgb([
            (x * 190 / w + noise) as u8,
            (y * 190 / h + noise) as u8,
            ((x + y) * 90 / (w + h) + noise) as u8,
        ])
    }))
}

/// [`noisy_rgb`] as a baseline JPEG.
pub fn noisy_jpeg(w: u32, h: u32) -> Vec<u8> {
    encode(&noisy_rgb(w, h), ImageFormat::Jpeg)
}
```

(`DynamicImage`, `RgbImage`, `Rgb` and `ImageFormat` are already imported there: `solid` and
`encode` use them.)

- [ ] **Step 3: Write the module with its tests, and `scaled_decode` stubbed**

Create `crates/photon-core/src/turbo.rs`. The functions are complete except `scaled_decode`,
which is stubbed to `None` so the tests can be seen to fail first:

```rust
//! libjpeg-turbo's scaled decode, for the preview a JPEG's thumbnail is made from.
//!
//! zune decodes every pixel of a photo, and `fit_within` then throws most of them away: a
//! 24 MP photo is decoded at 6000x4000 for a 1600x1067 preview. libjpeg-turbo can decode
//! straight at n/8 of the size, with a smaller inverse DCT per block. The same photo comes
//! out at 3/8 (2250x1500) in about three quarters of the time (measured in
//! `docs/superpowers/specs/2026-09-29-photon-turbo-jpeg-thumbnails-design.md`).
//!
//! It is a fast path, never the only one. Two kinds of file are `None`, and the caller then
//! decodes them with zune exactly as before:
//! - anything it does not take on: a colour space other than YCbCr, a header past `image`'s
//!   allocation limit;
//! - anything libjpeg complains about, even with a warning.
//!
//! libjpeg's answer to a damaged file is to warn and fill what it could not read with grey.
//! photon's answer stays whatever zune says, which is what it said before this module
//! existed.
//!
//! libjpeg reports errors and warnings through callbacks. The `mozjpeg` crate's own error
//! manager unwinds on an error and drops warnings, so this module installs its own, which
//! unwinds on either. Building that error manager is the module's only `unsafe` code.

use std::os::raw::c_int;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

use image::{DynamicImage, RgbImage};
use mozjpeg::Decompress;
use mozjpeg_sys::{J_COLOR_SPACE, jpeg_common_struct, jpeg_error_mgr, jpeg_std_error};

/// `image`'s default allocation limit (`Limits::default().max_alloc`), applied to the photo's
/// full size at RGBA8 as zune's path applies it. A header past it goes to zune, which refuses
/// it as it always has, so the per-decode bound `MAX_WORKERS` is reasoned against
/// (`decode_oriented`'s doc) is kept.
const MAX_DECODE_BYTES: u64 = 512 * 1024 * 1024;

/// The unwind payload for "libjpeg gave up or complained". Nothing reads it: any unwind out
/// of a decode means the same thing.
struct Refused;

/// `bytes`, a whole JPEG file, decoded as RGB at the smallest n/8 scale that covers `target`
/// on both axes, or `None`, in which case the caller decodes it with zune.
///
/// The picture is at least `target` and may be a pixel or so larger, since n/8 rounds up. The
/// caller resizes it to exactly `target`.
pub(crate) fn scaled_decode(bytes: &[u8], target: (u32, u32)) -> Option<DynamicImage> {
    let _ = (bytes, target);
    None
}

fn decode(bytes: &[u8], target: (u32, u32)) -> Option<DynamicImage> {
    let mut jpeg = Decompress::with_err(error_mgr()).from_mem(bytes).ok()?;
    // YCbCr only, which libjpeg assigns only to three components. zune returns greyscale as
    // `L8` and converts CMYK its own way; decoding either here would change a thumbnail's
    // pixel format or colours from what every earlier photon made.
    if jpeg.color_space() != J_COLOR_SPACE::JCS_YCbCr {
        return None;
    }
    let width = u32::try_from(jpeg.width()).ok()?;
    let height = u32::try_from(jpeg.height()).ok()?;
    if !within_decode_limit(width, height) {
        return None;
    }
    jpeg.scale(scale_for((width, height), target)?);
    let mut started = jpeg.rgb().ok()?;
    let out_width = u32::try_from(started.width()).ok()?;
    let out_height = u32::try_from(started.height()).ok()?;
    let pixels = started.read_scanlines::<u8>().ok()?;
    started.finish().ok()?;
    RgbImage::from_raw(out_width, out_height, pixels).map(DynamicImage::ImageRgb8)
}

fn within_decode_limit(width: u32, height: u32) -> bool {
    u64::from(width) * u64::from(height) * 4 <= MAX_DECODE_BYTES
}

/// The smallest n in 1..=8 whose n/8 scale covers `target` on both axes. `None` only when
/// even the full size does not, which a downscale's target never asks for.
fn scale_for((width, height): (u32, u32), (target_w, target_h): (u32, u32)) -> Option<u8> {
    (1..=8).find(|&n| scaled(width, n) >= target_w && scaled(height, n) >= target_h)
}

/// An edge at n/8, rounded up, as libjpeg computes it (`jdiv_round_up`).
fn scaled(edge: u32, n: u8) -> u32 {
    (u64::from(edge) * u64::from(n)).div_ceil(8) as u32
}

fn error_mgr() -> jpeg_error_mgr {
    // SAFETY: `jpeg_error_mgr` is a C struct of optional function pointers, integers and raw
    // pointers, for all of which zero is a valid value (`None`, 0, null), and
    // `jpeg_std_error` then fills in every field libjpeg reads.
    let mut err: jpeg_error_mgr = unsafe { std::mem::zeroed() };
    // SAFETY: `err` is a valid, exclusively borrowed `jpeg_error_mgr`. The call keeps no
    // pointer to it, so moving it afterwards (into `with_err`'s box) is sound.
    unsafe { jpeg_std_error(&mut err) };
    err.error_exit = Some(refuse);
    err.emit_message = Some(on_message);
    err
}

/// libjpeg's fatal error. It must not return: the `mozjpeg` reader aborts the process if it
/// does. `resume_unwind` rather than `panic!`, because it runs no panic hook: a file handed
/// back is not a bug, and must not log as one.
extern "C-unwind" fn refuse(_cinfo: &mut jpeg_common_struct) {
    resume_unwind(Box::new(Refused))
}

/// libjpeg's messages. Level -1 is a warning, about damaged data libjpeg would paper over
/// with grey, and it ends the decode like an error. Levels 0 and up are trace messages,
/// which are ignored.
extern "C-unwind" fn on_message(_cinfo: &mut jpeg_common_struct, level: c_int) {
    if level < 0 {
        resume_unwind(Box::new(Refused))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{encode, noisy_jpeg, noisy_rgb};
    use image::{ColorType, ImageFormat};
    use std::cell::Cell;

    /// The frame header's offset in a JPEG `image` wrote, which is baseline, so SOF0. Its
    /// quantisation tables, which come first, hold no 0xFF byte at `encode`'s quality, so the
    /// first `FF C0` is the marker.
    fn sof0(jpeg: &[u8]) -> usize {
        jpeg.windows(2)
            .position(|w| w == [0xFF, 0xC0])
            .expect("a baseline JPEG")
    }

    #[test]
    fn the_scale_is_the_smallest_that_covers_the_target() {
        // Camera sizes and their 1600 px previews, at the scales the spec measured.
        for (size, target, n) in [
            ((4032, 3024), (1600, 1200), 4),
            ((6000, 4000), (1600, 1067), 3),
            ((8256, 5504), (1600, 1067), 2),
            ((12800, 100), (1600, 13), 1),
            ((1601, 10), (1600, 10), 8),
            // 7/8 rounds the short edge up to 68 where the preview is 67: covered, and left
            // to the caller's resize.
            ((1828, 77), (1600, 67), 7),
        ] {
            assert_eq!(scale_for(size, target), Some(n), "{size:?}");
        }
        assert_eq!(scale_for((100, 100), (101, 100)), None);
    }

    #[test]
    fn a_plain_jpeg_decodes_at_the_scale_that_covers_its_target() {
        let img = scaled_decode(&noisy_jpeg(3300, 40), (1600, 19)).expect("decoded");
        assert_eq!(
            (img.width(), img.height(), img.color()),
            (1650, 20, ColorType::Rgb8)
        );
    }

    #[test]
    fn a_progressive_jpeg_decodes_at_the_scale_that_covers_its_target() {
        let mut bytes = Vec::new();
        let mut encoder = jpeg_encoder::Encoder::new(&mut bytes, 90);
        encoder.set_progressive(true);
        encoder
            .encode(
                noisy_rgb(3300, 40).to_rgb8().as_raw(),
                3300,
                40,
                jpeg_encoder::ColorType::Rgb,
            )
            .unwrap();
        let img = scaled_decode(&bytes, (1600, 19)).expect("decoded");
        assert_eq!((img.width(), img.height()), (1650, 20));
    }

    /// The files it does not take on. Each is one zune already decides, and must go on
    /// deciding.
    #[test]
    fn what_it_does_not_take_on_is_handed_back() {
        // Greyscale: libjpeg would happily make it RGB, which zune does not.
        let grey = encode(&noisy_rgb(3300, 40).grayscale(), ImageFormat::Jpeg);
        assert!(scaled_decode(&grey, (1600, 19)).is_none(), "greyscale");

        let mut cmyk = Vec::new();
        let pixels: Vec<u8> = noisy_rgb(330, 40)
            .to_rgb8()
            .as_raw()
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 30])
            .collect();
        jpeg_encoder::Encoder::new(&mut cmyk, 90)
            .encode(&pixels, 330, 40, jpeg_encoder::ColorType::Cmyk)
            .unwrap();
        assert!(scaled_decode(&cmyk, (160, 19)).is_none(), "CMYK");

        // Arithmetic coding (SOF9), which mozjpeg-sys builds without.
        let mut arithmetic = noisy_jpeg(3300, 40);
        let sof = sof0(&arithmetic);
        arithmetic[sof + 1] = 0xC9;
        assert!(scaled_decode(&arithmetic, (1600, 19)).is_none(), "arithmetic");

        // A header claiming 60000x60000: past the limit before a byte is decoded.
        let mut huge = noisy_jpeg(330, 40);
        let sof = sof0(&huge);
        huge[sof + 5..sof + 9].copy_from_slice(&[0xEA, 0x60, 0xEA, 0x60]);
        assert!(scaled_decode(&huge, (1600, 1600)).is_none(), "oversized");
    }

    /// Damaged data. libjpeg would warn about most of it and decode the rest as grey.
    #[test]
    fn a_warning_or_an_error_hands_the_file_back() {
        let whole = noisy_jpeg(3300, 40);
        let mut corrupt = whole.clone();
        let middle = corrupt.len() / 2;
        corrupt[middle..middle + 64].fill(0xFF);
        // Two stray bytes after a comment segment, which zune steps over.
        let stray = [
            &whole[..2],
            &[0xFF, 0xFE, 0x00, 0x03, b'x', 0x00, 0x00][..],
            &whole[2..],
        ]
        .concat();
        for (name, bytes) in [
            ("truncated", &whole[..whole.len() / 2]),
            ("corrupt", &corrupt[..]),
            ("stray bytes", &stray[..]),
            ("not a JPEG", &b"definitely not a jpeg"[..]),
        ] {
            assert!(scaled_decode(bytes, (1600, 19)).is_none(), "{name}");
        }
    }

    /// A file handed back is not a bug: it must not reach the panic hook, which logs one.
    /// Both of libjpeg's ways of ending a decode are covered: the cut file ends on the EOF
    /// warning (`on_message`), and the non-JPEG ends on a fatal header error (`refuse`).
    #[test]
    fn handing_a_file_back_runs_no_panic_hook() {
        thread_local! {
            static HOOKED: Cell<u32> = const { Cell::new(0) };
        }
        // The hook is process-wide, so it counts on the thread that panicked: another test
        // panicking meanwhile counts on its own thread, not this one.
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| HOOKED.with(|h| h.set(h.get() + 1))));
        let bytes = noisy_jpeg(330, 40);
        let warned = scaled_decode(&bytes[..bytes.len() / 2], (160, 19)).is_none();
        let failed = scaled_decode(b"definitely not a jpeg", (160, 19)).is_none();
        std::panic::set_hook(previous);
        assert!(warned && failed);
        assert_eq!(HOOKED.with(Cell::get), 0);
    }

    #[test]
    fn the_decode_limit_is_images() {
        // 16384 x 8192 x 4 is exactly 512 MiB.
        assert!(within_decode_limit(16384, 8192));
        assert!(!within_decode_limit(16385, 8192));
    }
}
```

In `crates/photon-core/src/lib.rs`, add `mod turbo;` on its own line directly after
`pub mod thumbs;`.

- [ ] **Step 4: Run the tests to see them fail**

Run: `cargo test -p photon-core --lib turbo`
Expected results:
- **FAIL:** `a_plain_jpeg_decodes_at_the_scale_that_covers_its_target` and
  `a_progressive_jpeg_decodes_at_the_scale_that_covers_its_target`, both with "decoded"
  (the stub returns `None`).
- **PASS:** the other five, because the stub hands everything back. Step 7's probes are what
  prove those five.
- **Warnings:** clippy/rustc warn that `decode` is unused. That's expected until Step 5.

- [ ] **Step 5: Implement `scaled_decode`**

Replace the stub's body:

```rust
pub(crate) fn scaled_decode(bytes: &[u8], target: (u32, u32)) -> Option<DynamicImage> {
    // The whole lifecycle runs inside the one catch, including the drop that destroys
    // libjpeg's state. An unwind out of any of it is a file handed back, never a panic that
    // reaches the thumbnail service's own `catch_unwind` as a decoder bug.
    catch_unwind(AssertUnwindSafe(|| decode(bytes, target)))
        .ok()
        .flatten()
}
```

- [ ] **Step 6: Run the tests to see them pass**

Run: `cargo test -p photon-core --lib turbo`
Expected: all 7 pass.

- [ ] **Step 7: Probe each rule (CLAUDE.md: a new test must fail with its rule removed)**

Make each change below with the Edit tool, run the command, check it FAILS as described, then
restore the exact original line with the Edit tool. Never use `sed`.

1. **The warning rule.** In `on_message`, change `if level < 0 {` to `if level < -1 {`.
   Run `cargo test -p photon-core --lib turbo::tests::a_warning_or_an_error_hands_the_file_back`.
   Expected: FAIL on "truncated", which now decodes with grey.
2. **The colour-space rule.** Delete the `if jpeg.color_space() != J_COLOR_SPACE::JCS_YCbCr { return None; }`
   block. Run `cargo test -p photon-core --lib turbo::tests::what_it_does_not_take_on_is_handed_back`.
   Expected: FAIL on "greyscale".
3. **The no-hook rule, error path.** In `refuse`, change `resume_unwind(Box::new(Refused))`
   to `panic!("refused")`. Run
   `cargo test -p photon-core --lib turbo::tests::handing_a_file_back_runs_no_panic_hook`.
   Expected: FAIL, with a nonzero count (the "not a JPEG" header error goes through `refuse`).
   Restore it, then do the same for the **warning path**: in `on_message`, change
   `resume_unwind(Box::new(Refused))` to `panic!("warned")`. Expected: FAIL, with a nonzero
   count (the cut file's EOF warning goes through `on_message`).
4. **The scale rule.** In `scale_for`, change `(1..=8).find(` to `(1..=8).rev().find(`. Run
   `cargo test -p photon-core --lib turbo::tests::the_scale_is_the_smallest_that_covers_the_target`.
   Expected: FAIL (8 where 4 is expected).

**Not probed, on purpose:** the decode limit's *use* in `decode` has no discriminating test. A
header over the limit is also a file whose data runs out, so libjpeg hands it back on the EOF
warning, after allocating the scaled buffer. Only `within_decode_limit` itself is tested. Say
so in the commit message, as CLAUDE.md asks.

- [ ] **Step 8: Run the gate, then commit**

Run the Rust gate (Global constraints). Expected: all pass.

```bash
git add crates/photon-core/Cargo.toml Cargo.lock crates/photon-core/src/lib.rs crates/photon-core/src/testutil.rs crates/photon-core/src/turbo.rs
git commit -F - <<'EOF'
feat(core): libjpeg-turbo's scaled JPEG decode, as a fast path

`turbo::scaled_decode` decodes a JPEG through libjpeg-turbo (the mozjpeg
crate) at the smallest n/8 scale covering a target size. It hands back
(`None`) every file it does not take on (greyscale, CMYK, arithmetic
coding, a header past `image`'s allocation limit) and every file libjpeg
warns or errors about, so the caller can decode those with zune as
before. Its own error manager unwinds on a warning too, which the
crate's drops; that is the module's only `unsafe`. Not wired in yet.

Probed: the warning rule (a truncated file then decodes), the colour-space
rule (greyscale then decodes), the unwind that skips the panic hook on
both the error and the warning path, and the scale search (reversed). The decode limit's use has no discriminating
test: a header over it is also a file whose data runs out, which libjpeg
hands back on the EOF warning anyway; `within_decode_limit` is tested
directly.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 2: `decode.rs`: route the preview through the fast path

**Files:**
- Modify: `crates/photon-core/src/decode.rs`: `decode_image` (:24-41), `decode_oriented`
  (:127-130), `fit_within_by` (:148-166), `fitted` (:172-177), and the tests module.

**Interfaces:**
- Consumes: `crate::turbo::scaled_decode(bytes: &[u8], target: (u32, u32)) -> Option<DynamicImage>` (Task 1).
- Consumes: `crate::jpeg::is_jpeg(head: &[u8]) -> bool` and
  `crate::jpeg::dimensions<R: BufRead + Seek>(r: &mut R) -> Option<(u32, u32)>`, both existing.
- Produces:
  - `pub(crate) enum PreviewDecoder { Turbo, Image }`, deriving `Debug, Clone, Copy, PartialEq, Eq`;
  - `pub(crate) fn preview_decode(path: &Path, max_edge: u32) -> Result<(DynamicImage, PreviewDecoder)>`;
  - `decode_oriented` keeps its public signature.

- [ ] **Step 1: Write the failing tests**

In `decode.rs`'s `mod tests`, extend the `crate::testutil` import to add `encode`,
`noisy_jpeg` and `noisy_rgb`. The current list is `avif_fixture, bmp_bytes, counted,
jpeg_bytes, jpeg_with_segments, tiff_bytes, write_file`. Then append these tests at the end of
the module:

```rust
    #[test]
    fn a_jpeg_larger_than_the_preview_is_decoded_by_libjpeg_turbo() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "wide.jpg", &noisy_jpeg(3300, 40));
        let (img, decoder) = preview_decode(&path, 1600).unwrap();
        assert_eq!(decoder, PreviewDecoder::Turbo);
        assert_eq!((img.width(), img.height()), fitted(3300, 40, 1600));
    }

    /// Everything but a large YCbCr JPEG is `image`'s, as every preview was before.
    #[test]
    fn everything_else_is_decoded_by_image() {
        let dir = tempfile::tempdir().unwrap();
        let whole = noisy_jpeg(3300, 40);
        let cases = [
            ("small.jpg", noisy_jpeg(400, 40)),
            ("wide.png", encode(&noisy_rgb(3300, 40), ImageFormat::Png)),
            (
                "grey.jpg",
                encode(&noisy_rgb(3300, 40).grayscale(), ImageFormat::Jpeg),
            ),
            ("cut.jpg", whole[..whole.len() / 2].to_vec()),
            ("a.avif", avif_fixture("irot90.avif")),
        ];
        for (name, bytes) in cases {
            let path = write_file(dir.path(), name, &bytes);
            // A cut JPEG may be an error from zune: still zune's answer, not the fast path's.
            let decoder = preview_decode(&path, 1600).map(|(_, decoder)| decoder);
            assert!(!matches!(decoder, Ok(PreviewDecoder::Turbo)), "{name}");
        }
    }

    /// At 7/8, 1828x77 comes out 1600x68, a pixel taller than the photo's 1600x67 preview,
    /// which every cached thumbnail of it has. Fitting the scaled picture would keep the 68.
    #[test]
    fn a_preview_decoded_at_a_scale_is_the_photos_own_fitted_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "odd.jpg", &noisy_jpeg(1828, 77));
        let (img, decoder) = preview_decode(&path, 1600).unwrap();
        assert_eq!(decoder, PreviewDecoder::Turbo);
        assert_eq!((img.width(), img.height()), (1600, 67));
    }

    /// A library's cache mixes thumbnails made by zune with ones made by libjpeg-turbo, and
    /// the look-alike check compares them with each other, so the two must make the same
    /// picture as far as `same_picture` can tell. That is the same bar, and the same reason,
    /// as `a_preview_is_the_same_picture_as_the_one_images_resize_made`.
    #[test]
    fn libjpeg_turbos_preview_is_the_same_picture_as_zunes() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "photo.jpg", &noisy_jpeg(1800, 300));
        let (ours, decoder) = preview_decode(&path, 1600).unwrap();
        assert_eq!(decoder, PreviewDecoder::Turbo);
        let theirs = fit_within(decode_image(&path).unwrap(), 1600);
        assert_eq!(
            (ours.width(), ours.height(), ours.color()),
            (theirs.width(), theirs.height(), theirs.color())
        );
        let grid = |preview: &DynamicImage| crate::similar::reduce(&preview.thumbnail(256, 256));
        let difference = crate::similar::picture_difference(&grid(&ours), &grid(&theirs));
        assert!(
            difference < crate::similar::SAME_PICTURE_MAX_DIFFERENCE / 4.0,
            "{difference}"
        );
    }

    /// Damaged data keeps today's answer exactly: zune's picture, or the `Error::Image` that
    /// marks the photo Failed rather than retrying it (`is_source_defect`, thumbs/service.rs).
    #[test]
    fn a_damaged_jpeg_previews_as_zune_decides_it() {
        let dir = tempfile::tempdir().unwrap();
        let whole = noisy_jpeg(3300, 40);
        let mut corrupt = whole.clone();
        let middle = corrupt.len() / 2;
        corrupt[middle..middle + 64].fill(0xFF);
        for (name, bytes) in [
            ("cut.jpg", whole[..whole.len() / 2].to_vec()),
            ("corrupt.jpg", corrupt),
        ] {
            let path = write_file(dir.path(), name, &bytes);
            let ours = preview_decode(&path, 1600);
            let theirs = decode_image(&path).map(|img| fit_within(img, 1600));
            match (ours, theirs) {
                (Ok((ours, decoder)), Ok(theirs)) => {
                    assert_eq!(decoder, PreviewDecoder::Image, "{name}");
                    assert!(ours.as_bytes() == theirs.as_bytes(), "{name}");
                }
                (Err(Error::Image(_)), Err(Error::Image(_))) => {}
                (ours, theirs) => panic!(
                    "{name}: {:?} against {:?}",
                    ours.map(|(_, decoder)| decoder),
                    theirs.map(|_| ())
                ),
            }
        }
    }
```

`ImageFormat`, `Error` and `avif_fixture` are already in scope there: `ImageFormat` through
`use super::*` (the top of `decode.rs` imports it), and `Error` and `avif_fixture` through the
module's own imports.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p photon-core --lib decode::tests`
Expected: a compile error, because `preview_decode` and `PreviewDecoder` are not defined. That
is not the proof. The proof is Step 5's probes, per CLAUDE.md. Go on to Step 3.

- [ ] **Step 3: Implement**

In `decode.rs`:

**(a)** Imports. Change `use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};` to
`use std::io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom};`, and add `use crate::turbo;`
after `use crate::jpeg;`.

**(b)** Split `decode_image` so that a reader can be handed in. Replace the whole function
(:24-41) with:

```rust
pub fn decode_image(path: &Path) -> Result<DynamicImage> {
    decode_from(BufReader::new(File::open(path)?), path)
}

/// [`decode_image`] over a reader already open on `path`'s contents: the file itself, or the
/// bytes [`preview_decode`] read to try libjpeg-turbo first and then handed back.
fn decode_from<R: BufRead + Seek>(mut reader: R, path: &Path) -> Result<DynamicImage> {
    // `fill_buf` peeks without consuming: the sniff costs no seek and no re-read, unlike a
    // take-and-rewind, which is what `read_header`'s "one open file, few syscalls" reasoning
    // (metadata.rs) needs from this on every describe().
    let is_avif = sniffed_avif(reader.fill_buf()?);
    if is_avif {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        return Ok(avif::decode_avif(&bytes)?);
    }
    let mut image = ImageReader::new(reader);
    // The extension is only the fallback when the bytes say nothing, as with
    // `ImageReader::open`, which this replaces so the file is opened once.
    if let Ok(format) = ImageFormat::from_path(path) {
        image.set_format(format);
    }
    Ok(image.with_guessed_format()?.decode()?)
}
```

Keep `decode_image`'s existing doc comment above it, unchanged.

**(c)** Replace `decode_oriented`'s body (:127-130), keeping its doc comment, and add the enum
and `preview_decode` directly after it:

```rust
pub fn decode_oriented(path: &Path, orientation: u8, max_edge: u32) -> Result<DynamicImage> {
    let (img, _) = preview_decode(path, max_edge)?;
    Ok(apply_orientation(img, orientation))
}

/// Which decoder made a preview. Only the tests read it: it is how they tell the fast path
/// from the fallback, whose pictures `same_picture` cannot tell apart, by design.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviewDecoder {
    /// libjpeg-turbo's scaled decode (`turbo`).
    Turbo,
    /// [`decode_image`] and [`fit_within`], as every preview was made before.
    Image,
}

/// `path` decoded and fitted within `max_edge`, not yet oriented, with which decoder made it.
/// It is [`decode_oriented`] without its last step.
///
/// A JPEG larger than `max_edge` goes to libjpeg-turbo first, which decodes it straight at the
/// smallest n/8 scale that covers the preview. Everything else, and every JPEG libjpeg-turbo
/// hands back, goes to [`decode_image`] and [`fit_within`] as before.
///
/// The file is read once either way. A JPEG is read whole, as `image`'s JPEG decoder reads it
/// anyway, and a handed-back JPEG is decoded from those same bytes. Any other format streams
/// from the file as it always did.
pub(crate) fn preview_decode(
    path: &Path,
    max_edge: u32,
) -> Result<(DynamicImage, PreviewDecoder)> {
    let mut reader = BufReader::new(File::open(path)?);
    if !jpeg::is_jpeg(reader.fill_buf()?) {
        let img = fit_within(decode_from(reader, path)?, max_edge);
        return Ok((img, PreviewDecoder::Image));
    }
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    // The target is the photo's own `fitted` size, not the scaled decode's: n/8 rounds up, so
    // fitting the scaled picture can land a pixel off the size every cached thumbnail of the
    // photo has.
    if let Some((width, height)) = jpeg::dimensions(&mut Cursor::new(bytes.as_slice()))
        && width.max(height) > max_edge
    {
        let target = fitted(width, height, max_edge);
        match turbo::scaled_decode(&bytes, target) {
            Some(img) => return Ok((resize_to(img, target), PreviewDecoder::Turbo)),
            // Logged for the smoke checklist's count of fallbacks: a warning late in a file
            // costs nearly a whole libjpeg decode before zune starts.
            None => tracing::debug!(
                path = %path.display(),
                "libjpeg-turbo handed a JPEG back to zune"
            ),
        }
    }
    let img = fit_within(decode_from(Cursor::new(bytes), path)?, max_edge);
    Ok((img, PreviewDecoder::Image))
}
```

**(d)** Split the resize out of `fit_within_by` (:148-166). Replace the whole function with
these three:

```rust
pub(crate) fn fit_within_by(img: DynamicImage, max_edge: u32, filter: FilterType) -> DynamicImage {
    if img.width().max(img.height()) <= max_edge {
        return img;
    }
    let size = fitted(img.width(), img.height(), max_edge);
    resize_by(img, size, filter)
}

/// `img` resized to exactly `size` with [`fit_within`]'s filter. It is for a picture decoded
/// at a scale near the size it must end up at (`turbo`), whose own `fitted` size could be a
/// pixel off the photo's.
fn resize_to(img: DynamicImage, size: (u32, u32)) -> DynamicImage {
    if (img.width(), img.height()) == size {
        return img;
    }
    resize_by(img, size, FilterType::Bilinear)
}

fn resize_by(img: DynamicImage, (width, height): (u32, u32), filter: FilterType) -> DynamicImage {
    let mut out = DynamicImage::new(width, height, img.color());
    // Alpha is resampled as it is stored, as `image` did, rather than premultiplied: that
    // would take a premultiplied copy of the whole source first, a second full-size buffer
    // for every transparent PNG, to change nothing for the JPEGs that are nearly every photo.
    let options = ResizeOptions::new()
        .resize_alg(ResizeAlg::Convolution(filter))
        .use_alpha(false);
    match Resizer::new().resize(&img, &mut out, &options) {
        Ok(()) => out,
        // Only a pixel layout the crate does not know refuses (`DynamicImage` is
        // non-exhaustive, so a future `image` can add one): slower, but still a thumbnail.
        Err(_) => img.resize(width, height, image::imageops::FilterType::Triangle),
    }
}
```

**(e)** Extend `decode_oriented`'s existing doc comment with one closing paragraph:

```rust
/// A JPEG is decoded by libjpeg-turbo at a reduced scale when it can be, and by `image` as
/// above otherwise: see [`preview_decode`], and `turbo` for why the scaled decode keeps the
/// same bound.
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p photon-core --lib decode`
Expected: all pass. That includes the existing `decode_never_upscales`,
`decode_reports_corrupt_and_missing_files`, `decodes_tiff_and_bmp`, `decodes_avif` and
`routes_by_content_not_extension`.

Then: `cargo test -p photon-core`
Expected: all pass, including `thumbs::cache`, `similar` and the thumbnail service. **If a
`similar` test fails, stop.** The spec says a moved existing test is a finding to explain,
not a threshold to loosen. Report it.

- [ ] **Step 5: Probe the new tests**

Make each change with the Edit tool, run the command, check it FAILS as described, and restore
the exact original.

1. **Fast path disconnected.** In `preview_decode`, change `match turbo::scaled_decode(&bytes, target) {`
   to `match None::<DynamicImage> {`. Run
   `cargo test -p photon-core --lib decode::tests::a_jpeg_larger_than_the_preview_is_decoded_by_libjpeg_turbo`.
   Expected: FAIL, `Image` != `Turbo`.
2. **Fitting the scaled picture.** Change `resize_to(img, target)` to
   `fit_within(img, max_edge)`. Run
   `cargo test -p photon-core --lib decode::tests::a_preview_decoded_at_a_scale_is_the_photos_own_fitted_size`.
   Expected: FAIL, `(1600, 68)` != `(1600, 67)`.
3. **Warnings accepted.** In `turbo.rs`'s `on_message`, change `if level < 0 {` to
   `if level < -1 {`. Run
   `cargo test -p photon-core --lib decode::tests::a_damaged_jpeg_previews_as_zune_decides_it`.
   Expected: FAIL on "cut.jpg" (`Turbo`, or mismatched arms).
4. **The picture wrong.** In `turbo.rs`'s `decode`, change `jpeg.rgb()` to
   `jpeg.to_colorspace(J_COLOR_SPACE::JCS_EXT_BGR)`. Run
   `cargo test -p photon-core --lib decode::tests::libjpeg_turbos_preview_is_the_same_picture_as_zunes`.
   Expected: FAIL on the difference. **If it passes**, the gradient is too weak to tell
   channels apart. In that case add a dark rectangle to the fixture, the way
   `a_preview_is_the_same_picture_as_the_one_images_resize_made` builds its block: overwrite
   pixels in `noisy_rgb(1800, 300).to_rgb8()` with `Rgb([200, 20, 20])` over
   `(400..900, 60..240)` before encoding. Then re-probe until it fails.

- [ ] **Step 6: Run the gate, then commit**

Run the Rust gate. Expected: all pass.

```bash
git add crates/photon-core/src/decode.rs
git commit -F - <<'EOF'
perf(core): decode a JPEG's preview with libjpeg-turbo at a reduced scale

`decode_oriented` now goes through `preview_decode`. A JPEG larger than the
preview goes to `turbo::scaled_decode` at the smallest n/8 scale covering
the photo's own fitted size, and is then resized to exactly that size,
because n/8 rounds up (1828x77 decodes at 7/8 to 1600x68, and its preview
is 1600x67). Everything else, and every JPEG libjpeg-turbo hands back,
is decoded by `image` as before, from the bytes already read.

Only the uncropped thumbnail changes: `decode_image` and every full-size
path are untouched. Decode and fit to 1600 px on arm64, 24 MP: 123 -> 93 ms
(spec 2026-09-29-photon-turbo-jpeg-thumbnails-design.md).

Probed: the fast path disconnected, the scaled picture fitted instead of
resized, warnings accepted, and the channels swapped. Each fails its test.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 3: Benchmark both preview paths

**Files:**
- Modify: `crates/photon-core/benches/render.rs` (the `thumbnail_24mp` group, :62-83)
- Modify: `docs/superpowers/specs/2026-09-29-photon-turbo-jpeg-thumbnails-design.md` (Testing,
  "Benchmark")

**Interfaces:**
- Consumes: the public `decode::decode_image`, `decode::fit_within` and
  `decode::decode_oriented`. `preview_decode` is `pub(crate)` and not visible to benches, so
  the stages compare the two paths through public functions.

- [ ] **Step 1: Add the stages**

In `render.rs`, directly after the existing `g.bench_function("decode_oriented", ...)` block
and before `g.finish();`, add:

```rust
    // The two preview paths side by side, at orientation 1 so nothing but the decode and the
    // fit differs: zune's full decode then `fit_within`, and `decode_oriented`, which takes
    // libjpeg-turbo's scaled decode for this JPEG.
    g.bench_function("preview_zune", |b| {
        b.iter(|| {
            black_box(decode::fit_within(
                decode::decode_image(&path).unwrap(),
                PREVIEW_EDGE,
            ))
        })
    });
    g.bench_function("preview_turbo", |b| {
        b.iter(|| black_box(decode::decode_oriented(&path, 1, PREVIEW_EDGE).unwrap()))
    });
```

- [ ] **Step 2: Run it**

Run: `cargo bench -p photon-core --bench render -- thumbnail_24mp/preview`
Expected: both stages report. On Apple Silicon, `preview_turbo` should be roughly 25% under
`preview_zune`: the spec measured 93 against 123 ms with this fixture. Keep the output for
the PR description.

- [ ] **Step 3: Amend the spec**

In the spec's "Testing" section, replace the "**Benchmark.**" paragraph with:

```markdown
**Benchmark.** `benches/render.rs`'s `thumbnail_24mp` group gains `preview_zune` (zune's
decode then `fit_within`) and `preview_turbo` (`decode_oriented` at orientation 1, which takes
the fast path). `preview_decode` is `pub(crate)`, out of a bench's reach, so the two paths are
compared through public functions. The CI gate in CLAUDE.md compiles only the grid bench, so
the PR also runs `cargo bench -p photon-core --bench render --no-run` once.
```

- [ ] **Step 4: Run the gate and the render bench build, then commit**

Run the Rust gate, plus `cargo bench -p photon-core --bench render --no-run`. Expected: all
pass.

```bash
git add crates/photon-core/benches/render.rs docs/superpowers/specs/2026-09-29-photon-turbo-jpeg-thumbnails-design.md
git commit -m "bench(render): the zune and libjpeg-turbo preview paths side by side

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: nasm on every CI and release runner

**Files:**
- Modify: `.github/workflows/ci.yml` (the `rust` job, after its Linux-dependencies step, :18-22)
- Modify: `.github/workflows/release.yml` (the `linux`, `macos` and `windows` jobs)
- Modify: `docs/superpowers/specs/2026-09-29-photon-turbo-jpeg-thumbnails-design.md` (Build,
  "nasm in CI")

**Why no third-party action.** The spec named `ilammy/setup-nasm`. Each runner's own package
manager installs nasm in one line, and nasm's official macOS binaries are x86_64, which an
arm64 runner may not be able to run without Rosetta. Homebrew's nasm is native. The spec is
amended in Step 4.

**Why a cached build can't hide a plain-C one.** mozjpeg-sys reruns its build script only when
its vendored sources change, not when nasm appears. Nothing is cached for this dependency yet,
because it is new in this branch and nothing has been pushed. So nasm has to be in the
workflows before the first push, which is this task, ahead of Task 5's push.

- [ ] **Step 1: CI**

In `.github/workflows/ci.yml`, in the `rust` job, add these two steps directly after the
"Install Linux webview dependencies" step and before `- uses: dtolnay/rust-toolchain@stable`:

```yaml
      # libjpeg-turbo's x86 SIMD code is nasm assembly. Without nasm, mozjpeg-sys builds plain
      # C instead, silently, so the next step fails by name if nasm is missing.
      - name: Install nasm
        shell: bash
        run: |
          case "$RUNNER_OS" in
            Linux)   sudo apt-get update && sudo apt-get install -y nasm ;;
            macOS)   brew install nasm ;;
            Windows) choco install nasm -y --no-progress && echo 'C:\Program Files\NASM' >> "$GITHUB_PATH" ;;
          esac
      - name: nasm is on the PATH
        run: nasm -v
```

- [ ] **Step 2: Release**

In `.github/workflows/release.yml`, add the same two steps, with the same comment, to three
jobs:
- **`linux`:** directly after its "Install Linux webview dependencies" step.
- **`macos`:** directly after `- uses: actions/checkout@v5`. The x86_64 `.dmg` is
  cross-compiled on this arm64 runner, and it is the one that needs nasm.
- **`windows`:** directly after `- uses: actions/checkout@v5`.

Do not add them to the versions job at the top of the file, which builds no code.

- [ ] **Step 3: Check the YAML parses**

Run: `python3 -c "import yaml,sys; [yaml.safe_load(open(f)) for f in sys.argv[1:]]" .github/workflows/ci.yml .github/workflows/release.yml && echo ok`
Expected: `ok`. If PyYAML is missing, run `ruby -ryaml -e 'ARGV.each { |f| YAML.load_file(f) }' .github/workflows/ci.yml .github/workflows/release.yml && echo ok`.

- [ ] **Step 4: Amend the spec**

In the spec's "Build" section, replace the "**nasm in CI.**" bullet with:

```markdown
- **nasm in CI.** In `.github/workflows/ci.yml`, the `rust` job installs nasm on all three
  runners with the runner's own package manager: `apt-get` on Linux, Homebrew on macOS,
  Chocolatey on Windows (adding `C:\Program Files\NASM` to the PATH). It then runs `nasm -v`
  as its own step, so a missing nasm fails the job by name instead of silently producing a
  plain-C build. There is no third-party action: nasm's official macOS binaries are x86_64,
  and Homebrew's is native to the arm64 runners.
```

- [ ] **Step 5: Run the gate, then commit**

Run the Rust gate. Expected: all pass. It checks nothing in YAML; CI does, in Task 5.

```bash
git add .github/workflows/ci.yml .github/workflows/release.yml docs/superpowers/specs/2026-09-29-photon-turbo-jpeg-thumbnails-design.md
git commit -F - <<'EOF'
ci: nasm on every Rust runner, for libjpeg-turbo's x86 SIMD

mozjpeg-sys builds plain C when nasm is missing, without a word; a
`nasm -v` step makes that a named failure instead. Installed with each
runner's own package manager.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 5: Measure on x86 before merging

This gates the merge (spec, "Build"). A temporary workflow runs the render bench on x86 Linux
and Windows, with and without nasm, and is deleted afterwards.

**Files:**
- Create, then delete: `.github/workflows/bench-x86.yml`

- [ ] **Step 1: Add the temporary workflow**

```yaml
# TEMPORARY - measures libjpeg-turbo's preview path on x86 for the PR, then is deleted
# (docs/superpowers/plans/2026-09-29-photon-turbo-jpeg-thumbnails.md, Task 5).
name: bench-x86
on:
  push:
    branches: [feat/turbo-jpeg-thumbnails]
jobs:
  bench:
    name: bench (${{ matrix.os }}, nasm ${{ matrix.nasm }})
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest]
        nasm: [with, without]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v5
      - name: Install nasm
        if: matrix.nasm == 'with'
        shell: bash
        run: |
          case "$RUNNER_OS" in
            Linux)   sudo apt-get update && sudo apt-get install -y nasm ;;
            Windows) choco install nasm -y --no-progress && echo 'C:\Program Files\NASM' >> "$GITHUB_PATH" ;;
          esac
      - name: No nasm, for the plain-C build
        if: matrix.nasm == 'without'
        shell: bash
        run: "! command -v nasm"
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo bench -p photon-core --bench render -- thumbnail_24mp/preview
```

- [ ] **Step 2: Commit and push**

Run the Rust gate first.

```bash
git add .github/workflows/bench-x86.yml
git commit -m "ci: TEMPORARY x86 bench of the libjpeg-turbo preview path

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
git push -u origin feat/turbo-jpeg-thumbnails
```

This push also runs the real CI (`ci.yml`) on the branch. Watch it the same way.

- [ ] **Step 3: Read the numbers**

```bash
gh run list --branch feat/turbo-jpeg-thumbnails --workflow bench-x86.yml --limit 1
gh run watch <run-id>
gh run view <run-id> --log | grep -E "preview_(zune|turbo)" -A2
```

Record, for each of the four jobs, the median `time:` of `preview_zune` and `preview_turbo`.

- [ ] **Step 4: Decide**

- **Continue** if, on both Linux and Windows *with nasm*, `preview_turbo` beats `preview_zune`.
- **Otherwise STOP and report the four pairs to the user.** The spec's fallback is an
  arm64-only fast path through a target-specific dependency, which is a spec amendment for
  the user to approve.
- **Also check the real CI.** `gh run list --branch feat/turbo-jpeg-thumbnails --workflow ci.yml`
  must pass on all three OSes. The Windows run of `turbo::tests` and
  `decode::tests::a_damaged_jpeg_previews_as_zune_decides_it` is what proves that libjpeg
  unwinds under MSVC. **If Windows aborts** ("process didn't exit successfully", with no test
  failure listed), STOP and report it: the unwind does not cross MSVC-built C, and the design
  needs another way to end a decode.

- [ ] **Step 5: Delete the workflow**

```bash
git rm .github/workflows/bench-x86.yml
git commit -m "ci: remove the temporary x86 bench

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

Keep the four pairs of numbers for the PR description (Task 7).

---

### Task 6: Documentation and notices

**Files:**
- Modify: `README.md` (:55-61, "File formats")
- Modify: `site/index.html` (:176-177)
- Modify: `CLAUDE.md` (:579-584, Conventions)
- Modify: `THIRD-PARTY-NOTICES.md` (a new section at the end)
- Modify: `docs/smoke-checklist.md` (a new item)

- [ ] **Step 1: README**

In `README.md`, replace:

```
1600-pixel preview elsewhere. Camera RAW files and HEIC are not read: every way of decoding
them means shipping a C library, and photon deliberately has no native dependencies.
```

with:

```
1600-pixel preview elsewhere. JPEG thumbnails are made with libjpeg-turbo, which decodes a
photo straight at the size a thumbnail needs. Camera RAW files and HEIC are not read:
decoding them means shipping a large C library, and photon keeps its C to a few small,
vendored ones (SQLite, libwebp, libjpeg-turbo), compiled in, so it needs no system libraries.
```

- [ ] **Step 2: Website**

In `site/index.html`, replace:

```html
    <li>No camera RAW or HEIC. Reading them means shipping a C library, and photon has no native
      dependencies.</li>
```

with:

```html
    <li>No camera RAW or HEIC. Reading them means shipping a large C library, and photon keeps
      its C to a few small ones compiled into it, so it needs no system libraries.</li>
```

- [ ] **Step 3: CLAUDE.md**

Replace:

```
- **No native library dependencies.** Nothing wrapping a C/C++ SDK. This is what made packaging
  tractable on three platforms, and it is why XMP and INI parsing are hand-rolled or pure-Rust.
```

with:

```
- **No system library dependencies.** photon's C is vendored and compiled in with `cc`:
  SQLite, libwebp and libjpeg-turbo (the mozjpeg crate, for the scaled decode of a JPEG's
  preview, `photon_core::turbo`). Nothing wrapping a C/C++ SDK: that bar is what made
  packaging tractable on three platforms, and it is why XMP and INI parsing are hand-rolled or
  pure-Rust. nasm is a build tool on the CI and release runners only, for libjpeg-turbo's x86
  SIMD code; a build without it still works, as plain C. A new C dependency is a spec-level
  decision (`2026-09-29-photon-turbo-jpeg-thumbnails-design.md` is the worked example).
```

Then, in the AVIF bullet, replace:

```
  `avif/av1.rs` holds the only `unsafe` code in photon-core. Every full decode goes through
  `decode::decode_image`, which sniffs the `ftyp` box; calling `ImageReader` directly skips
```

with:

```
  `avif/av1.rs` and `turbo.rs` (libjpeg's error manager) hold the only `unsafe` code in
  photon-core. Every full decode goes through `decode::decode_image`, which sniffs the `ftyp`
  box; the uncropped thumbnail's preview goes through `decode::preview_decode`, which tries
  libjpeg-turbo's scaled decode on a JPEG first and hands everything else to
  `decode_image`'s own path. Calling `ImageReader` directly skips
```

Read the lines after it to check the sentence still flows: it should continue "AVIF. The
container's `irot`/`imir` ...".

- [ ] **Step 4: Notices**

Append to `THIRD-PARTY-NOTICES.md`:

````markdown
## libjpeg-turbo (mozjpeg)

`crates/photon-core` depends on mozjpeg 0.10.13 and mozjpeg-sys 2.2.3
(https://github.com/kornelski/mozjpeg-sys), which compile in mozjpeg 4.1.5
(https://github.com/mozilla/mozjpeg), a fork of libjpeg-turbo, to decode a JPEG's thumbnail at
a reduced scale. Licensed "IJG AND Zlib AND BSD-3-Clause". As the IJG licence asks of a program
distributed as executable code: **this software is based in part on the work of the
Independent JPEG Group.** The IJG terms are reproduced in full under jpeg-encoder, above. The
Modified BSD licence, which covers libjpeg-turbo's own code, and the zlib licence, which covers
its SIMD extensions, follow.

```text
Copyright (C)2009-2023 D. R. Commander.  All Rights Reserved.
Copyright (C)2015 Viktor Szathmáry.  All Rights Reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

- Redistributions of source code must retain the above copyright notice,
  this list of conditions and the following disclaimer.
- Redistributions in binary form must reproduce the above copyright notice,
  this list of conditions and the following disclaimer in the documentation
  and/or other materials provided with the distribution.
- Neither the name of the libjpeg-turbo Project nor the names of its
  contributors may be used to endorse or promote products derived from this
  software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS",
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
ARE DISCLAIMED.  IN NO EVENT SHALL THE COPYRIGHT HOLDERS OR CONTRIBUTORS BE
LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
POSSIBILITY OF SUCH DAMAGE.
```

```text
Copyright 2009 Pierre Ossman <ossman@cendio.se> for Cendio AB
Copyright (C) 2010, 2016, 2018-2019, D. R. Commander.
Copyright (C) 2018, Matthieu Darbois.
Copyright (C) 2018, Matthias Räncker.

Based on the x86 SIMD extension for IJG JPEG library - version 1.02

Copyright (C) 1999-2006, MIYASAKA Masaru.

This software is provided 'as-is', without any express or implied
warranty.  In no event will the authors be held liable for any damages
arising from the use of this software.

Permission is granted to anyone to use this software for any purpose,
including commercial applications, and to alter it and redistribute it
freely, subject to the following restrictions:

1. The origin of this software must not be misrepresented; you must not
   claim that you wrote the original software. If you use this software
   in a product, an acknowledgment in the product documentation would be
   appreciated but is not required.
2. Altered source versions must be plainly marked as such, and must not be
   misrepresented as being the original software.
3. This notice may not be removed or altered from any source distribution.
```
````

The BSD text is copied from mozjpeg's `LICENSE.md` at `c2bc351`, with its `<br>` dropped. The
zlib text is the header of `simd/nasm/jsimdext.inc` at the same commit.

- [ ] **Step 5: Smoke checklist**

In `docs/smoke-checklist.md`, add this item directly after the item that begins
"- [ ] Thumbnails appear within seconds":

```markdown
- [ ] **JPEG thumbnails through libjpeg-turbo.** Point photon at a copy of a folder of a few
  hundred real camera JPEGs (several cameras and a phone if you have them), with a fresh
  profile, on the previous release and then this one, with `RUST_LOG=photon_core=debug`.
  Note both imports' times until the last thumbnail is ready. On this release the thumbnails
  and the viewer's previews look as they did. Count the log's "libjpeg-turbo handed a JPEG
  back to zune" lines. More than a few percent of any one camera's files means a warning is
  sending them back after a nearly whole decode, and that goes back to the spec before
  release.
```

- [ ] **Step 6: Run the gates, then commit**

Run the Rust gate. Also run `npm run check` and `npm test` from the repo root: this task
touches no UI code, but `site/` and the docs do not break it either way, and it costs little.
Expected: all pass.

```bash
git add README.md site/index.html CLAUDE.md THIRD-PARTY-NOTICES.md docs/smoke-checklist.md
git commit -F - <<'EOF'
docs: libjpeg-turbo in the README, site, CLAUDE.md and notices

"No native dependencies" becomes "no system libraries": the C photon uses
(SQLite, libwebp, libjpeg-turbo) is vendored and compiled in. The notices
carry libjpeg-turbo's BSD and zlib licences; its IJG terms are already
there under jpeg-encoder. The smoke checklist gains a real-camera import
that counts the files libjpeg-turbo hands back.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 7: Independent review, then the PR

CLAUDE.md: "A large branch gets an independent read before it merges ... Point the reviewer at
the effect wiring and at anything a new feature *arms* in old code."

- [ ] **Step 1: Request an independent review**

Use superpowers:requesting-code-review on `main..feat/turbo-jpeg-thumbnails`. Point the
reviewer at:
- **`preview_decode`.** A JPEG is now read whole before its type is known to decode: a large
  file's memory, and the fallback decoding a `Cursor` copy. Does anything else that used to
  stream now buffer?
- **`resize_to` and `fit_within_by`.** Are the rounding and the no-op case the same as before
  for every existing caller of `fit_within`?
- **`turbo.rs`'s unwind.** Can an unwind escape `catch_unwind`, for example from `Drop`? Is
  anything left borrowed or half-initialised in libjpeg after a warning unwinds mid-scanline?
  It is destroyed by `Decompress`'s drop inside the catch.
- **Old code this arms.** The thumbnail service's `catch_unwind` and in-flight guard now
  surround C code. Also `is_source_defect`'s `Error::Image` versus `IoError`: a fallback must
  never turn a damaged file into an I/O error that is retried forever.

Fix what the review finds, each fix with its own probe, and run the gate before each commit.

- [ ] **Step 2: Wait for CI**

```bash
git push
gh pr create --base main --title "perf: JPEG thumbnails through libjpeg-turbo's scaled decode" --body-file <file>
gh pr checks <N> --watch
```

Straight after a push, `gh pr checks` can answer "no checks reported". Wait until checks exist
before trusting it (CLAUDE.md).

The PR body must contain:
- what changed, and the spec's link;
- the arm64 numbers from Task 3;
- the four x86 pairs from Task 5;
- the security check's result from Task 0;
- the tests whose rules were probed, and the one that could not be (the decode limit's use,
  from Task 1);
- the smoke-checklist item as still to run.

End it with the line `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.

- [ ] **Step 3: Hand back**

Report the PR's URL and its CI state to the user. Do not merge: the smoke-checklist import is
still to run, on real camera files.
