# AVIF Photos Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Index, thumbnail, edit and view `.avif` photos, decoded in pure Rust.

**Architecture:** A new `photon_core::avif` module parses the container with `zenavif-parse`,
decodes each AV1 item with `rav1d` through a small `unsafe` wrapper (`avif/av1.rs`), converts
YUV to RGB itself (`avif/yuv.rs`), then stitches grid tiles, attaches alpha and applies the
container's `clap`/`irot`/`imir`. `decode.rs` gains one routing entry point, `decode_image`,
which sniffs the `ftyp` box and sends AVIFs there and everything else to `image`. Its three
callers are `decode_oriented`, `edit::render_full` and the dimensions read in `metadata.rs`.
The webview is served the file itself as `image/avif`.

**Tech Stack:** Rust 2024, `image` 0.25.10, `rav1d` 1.1.0 (no default features),
`zenavif-parse` 0.6.2, `libc` 0.2, `kamadak-exif` 0.6.1 (already present).

**Spec:** `docs/superpowers/specs/2026-09-24-photon-avif-design.md`

## Global Constraints

- No native library dependencies: `rav1d` is built with `default-features = false,
  features = ["bitdepth_8", "bitdepth_16"]`, so no assembly, no `nasm`, no C compiler.
- All `unsafe` code lives in `crates/photon-core/src/avif/av1.rs`, and every `unsafe` block
  carries a `// SAFETY:` comment.
- rav1d settings: `n_threads = 1`, `max_frame_delay = 1`, `frame_size_limit = 512 MiB / 4`.
- Per-decode memory bound: 512 MiB (`MAX_DECODE_BYTES`), the same as `image`'s default limits.
- No `EXIF_VERSION` bump, no schema migration.
- For AVIF, `describe()` stores `orientation = 1` and the *displayed* dimensions (after
  `clap` and `irot`).
- `mime_for("avif")` is `image/avif`; the unedited file is served as-is.
- Workspace `rust-version` rises from `1.88` to `1.93` (`zenavif-parse`'s minimum).
- The Rust gate (CLAUDE.md) must pass before every commit: `cargo fmt --all`,
  `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`.
- Every new test is watched failing with its change reverted (CLAUDE.md). A compile error
  does not count as that failure. Each task names the probes to run.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.

## Review Focus

1. **An odd-sized 4:2:0 photo** (33×17): the chroma plane rounds up. It must decode at 33×17
   without indexing past the chroma plane. Pinned by `odd_420.avif` in Task 3.
2. **A truncated or corrupt AVIF** should give the failed-thumbnail placeholder (an
   `Error::Image` that `is_source_defect` accepts), never a panic or a hang in the
   send/get loop. Pinned by `a_truncated_avif_is_an_error_not_a_panic` (Task 3) and
   `decode_reports_a_corrupt_avif_as_an_image_error` (Task 4).
3. **A phone photo with EXIF orientation 6 *and* `irot`**, which is how libavif writes one,
   must show upright once, not rotated twice. Pinned by `exif_orientation6.avif` in Task 4's
   `describe` test.
4. **A file whose extension lies**: a JPEG named `.avif` still decodes, because routing is by
   content. Pinned in Task 4.
5. **A `mif1`-major-brand AVIF**: `zenavif-parse` refuses it even in lenient mode, so the
   in-memory brand rewrite must make it decode. Pinned in Task 3.

---

## File Structure

| File | Responsibility |
|---|---|
| `Cargo.toml` (workspace) | `rust-version = "1.93"` |
| `crates/photon-core/Cargo.toml` | `rav1d`, `zenavif-parse`, `libc` |
| `crates/photon-core/testdata/avif/*.avif` | 13 fixtures made with `avifenc` |
| `crates/photon-core/testdata/avif/README.md` | how each fixture was made, and what it holds |
| `crates/photon-core/src/avif/av1.rs` | the only `unsafe`: one AV1 item → `Planes` |
| `crates/photon-core/src/avif/yuv.rs` | `Planes` → `RgbImage`, alpha attach |
| `crates/photon-core/src/avif/mod.rs` | sniffing, parsing, grid, transforms, dimensions |
| `crates/photon-core/src/lib.rs` | `pub mod avif;` |
| `crates/photon-core/src/testutil.rs` | `avif_fixture(name)` |
| `crates/photon-core/src/decode.rs` | `decode_image`, `dimensions`; `decode_oriented` uses them |
| `crates/photon-core/src/edit.rs` | `render_full` uses `decode_image` |
| `crates/photon-core/src/metadata.rs` | dimensions via `decode::dimensions`, orientation pin |
| `crates/photon-core/src/media.rs` | `"avif"` extension |
| `crates/photon-core/src/scanner.rs` | indexing test |
| `crates/photon-app/src/protocol.rs` | `image/avif` |
| `README.md` | formats section, smoke checklist |
| `CLAUDE.md` | one paragraph: where AVIF is decoded and why |

---

### Task 1: Dependencies, fixtures, and the AV1 wrapper

**Files:**
- Modify: `Cargo.toml` (workspace, line 8)
- Modify: `crates/photon-core/Cargo.toml`
- Create: `crates/photon-core/testdata/avif/` (13 `.avif` files + `README.md`)
- Create: `crates/photon-core/src/avif/mod.rs` (stub, grown in Task 3)
- Create: `crates/photon-core/src/avif/av1.rs`
- Modify: `crates/photon-core/src/lib.rs`
- Modify: `crates/photon-core/src/testutil.rs`

**Interfaces:**
- Produces: `pub(crate) struct av1::Planes { width: usize, height: usize, depth: u8, mono: bool, shift: (u32, u32), y: Vec<u16>, u: Vec<u16>, v: Vec<u16>, matrix: u16, full_range: bool }`
  with `fn chroma_width(&self) -> usize` and `fn chroma_height(&self) -> usize`;
  `pub(crate) fn av1::decode(obu: &[u8]) -> Result<Planes, String>`;
  `pub fn testutil::avif_fixture(name: &str) -> Vec<u8>` (test-only).

- [ ] **Step 1: Raise the workspace minimum Rust version**

In `Cargo.toml` change `rust-version = "1.88"` to `rust-version = "1.93"`. `zenavif-parse`
declares 1.93. CI and `mise.toml` build on stable, so only the declared number moves.

- [ ] **Step 2: Add the dependencies**

In `crates/photon-core/Cargo.toml`, `[dependencies]`, keep the alphabetical order:

```toml
libc = "0.2"
rav1d = { version = "1.1.0", default-features = false, features = ["bitdepth_8", "bitdepth_16"] }
zenavif-parse = "0.6.2"
```

`libc` is already in `Cargo.lock` (0.2.189). It is needed for `EAGAIN`, which is 11 on Linux
and Windows but 35 on macOS.

Run: `cargo build -p photon-core`
Expected: builds. rav1d compiles on stable Rust; its own `rust-toolchain.toml` does not apply
when it is built as a dependency.

- [ ] **Step 3: Make the fixtures**

This needs `avifenc` (libavif ≥ 1.0) and ImageMagick's `magick`. Run from the repo root:

```bash
set -e
D=crates/photon-core/testdata/avif; mkdir -p "$D"; T=$(mktemp -d)
magick -size 32x32 xc:'#ff0000' -size 32x32 xc:'#0000ff' +append -depth 8 "$T/red_blue.png"
magick \( -size 64x64 xc:'#ff0000' xc:'#00ff00' +append \) \( -size 64x64 xc:'#0000ff' xc:'#ffffff' +append \) -append -depth 8 "$T/quads.png"
magick -size 32x32 xc:black xc:white +append -depth 8 "$T/black_white.png"
magick -size 16x32 xc:'rgba(255,0,0,0.5)' -size 16x32 xc:'rgba(255,0,0,1)' +append -depth 8 "$T/alpha.png"
magick -size 33x17 xc:'#ff0000' -depth 8 "$T/odd.png"
magick -size 32x16 xc:'#ff0000' -size 32x16 xc:'#0000ff' -append -depth 8 "$T/top_bottom.png"
magick -size 16x32 xc:'#00ff00' -size 32x32 xc:'#ff0000' -size 16x32 xc:'#0000ff' +append -depth 8 "$T/green_red_blue.png"
python3 - "$T/exif6.bin" <<'EOF'
import struct, sys
# Big-endian TIFF: IFD0 {Orientation=6, ExifIFDPointer} -> Exif IFD {DateTimeOriginal}
dt = b"2024:06:15 12:30:45\0"
exif_off = 8 + 2 + 2 * 12 + 4
dt_off = exif_off + 2 + 12 + 4
ifd0 = struct.pack(">H", 2) + struct.pack(">HHIHH", 0x0112, 3, 1, 6, 0) \
     + struct.pack(">HHII", 0x8769, 4, 1, exif_off) + struct.pack(">I", 0)
exif = struct.pack(">H", 1) + struct.pack(">HHII", 0x9003, 2, len(dt), dt_off) + struct.pack(">I", 0)
open(sys.argv[1], "wb").write(b"MM\x00\x2a" + struct.pack(">I", 8) + ifd0 + exif + dt)
EOF
E="avifenc -s 8 -q 90"
$E -y 420 --cicp 1/13/1 -r limited "$T/red_blue.png" "$D/red_blue_709_limited.avif"
$E -y 420 --cicp 1/13/1 -r limited --icc /usr/share/ghostscript/iccprofiles/srgb.icc "$T/red_blue.png" "$D/icc_709_limited.avif"
$E -y 444 --cicp 1/13/6 -r full "$T/red_blue.png" "$D/red_blue_444_full.avif"
$E -d 10 -y 420 --grid 2x2 "$T/quads.png" "$D/grid_10bit.avif"
$E -y 420 --irot 1 "$T/red_blue.png" "$D/irot90.avif"
$E -y 420 --imir 1 "$T/red_blue.png" "$D/imir.avif"
$E -y 420 --exif "$T/exif6.bin" "$T/red_blue.png" "$D/exif_orientation6.avif"
$E -y 444 "$T/alpha.png" "$D/alpha.avif"
$E -y 400 "$T/black_white.png" "$D/mono.avif"
$E -y 420 "$T/odd.png" "$D/odd_420.avif"
$E -y 422 "$T/top_bottom.png" "$D/top_bottom_422.avif"
$E -y 444 --crop 16,0,32,32 "$T/green_red_blue.png" "$D/clap.avif"
ls -la "$D"
```

Any sRGB ICC profile will do for `--icc`. If ghostscript's is missing, use another one and
record which in the README. Expected: twelve files, each a few hundred bytes except the ICC
one (about 3 KB). The thirteenth case (`mif1`) is patched from `red_blue_709_limited.avif`
inside the test, so it needs no file.

Verify the transforms landed. `avifdec --info` (from libavif) prints them:

```bash
for f in irot90 imir exif_orientation6 grid_10bit clap; do avifdec --info $D/$f.avif | grep -iE 'Resolution|Bit Depth|irot|imir|clap|Exif'; done
```

Expected: `irot90` rotation 1, `imir` mirror 1, `exif_orientation6` **rotation 3 and Exif
present** (libavif turns EXIF orientation 6 into `irot` 3, as a phone's encoder does),
`grid_10bit` 128x128 at 10 bits, and `clap` with a clap property.

- [ ] **Step 4: Write the fixtures' README**

Create `crates/photon-core/testdata/avif/README.md`:

````markdown
# AVIF test fixtures

Made with `avifenc` (libavif 1.4.2, aom encoder) and ImageMagick. AV1 cannot be written by
hand the way the TIFF and BMP fixtures in `testutil.rs` are, and encoding in the test would
need `image`'s `avif` encoder (rav1e) as a dev-dependency. Every picture is flat colour so a
test can assert *where* a colour lands, not just the size. The expected colours in the tests
are the source colours, within ±12: lossy AV1 moves a flat blue to about 243 under libavif.

| File | Size | Holds | Made with |
|---|---|---|---|
| `red_blue_709_limited.avif` | 64×32 | red left, blue right; BT.709, limited range, 4:2:0 | `-y 420 --cicp 1/13/1 -r limited` |
| `icc_709_limited.avif` | 64×32 | as above, but `colr` is an ICC profile, so the matrix is only in the AV1 sequence header | as above plus `--icc srgb.icc` |
| `red_blue_444_full.avif` | 64×32 | red left, blue right; BT.601, full range, 4:4:4 | `-y 444 --cicp 1/13/6 -r full` |
| `grid_10bit.avif` | 128×128 | 2×2 grid of 64×64 tiles, 10-bit: red, green / blue, white | `-d 10 -y 420 --grid 2x2` |
| `irot90.avif` | 32×64 shown | red_blue with `irot` 1 (90° anticlockwise): blue on top | `--irot 1` |
| `imir.avif` | 64×32 | red_blue with `imir` 1: blue left, red right | `--imir 1` |
| `exif_orientation6.avif` | 32×64 shown | red_blue with EXIF orientation 6 and date 2024:06:15 12:30:45, which libavif also wrote as `irot` 3: red on top | `--exif exif6.bin` |
| `alpha.avif` | 32×32 | red; alpha 128 left half, 255 right half | `-y 444` from an RGBA PNG |
| `mono.avif` | 64×32 | black left, white right; 4:0:0 | `-y 400` |
| `odd_420.avif` | 33×17 | red; odd size, so the 4:2:0 chroma plane rounds up | `-y 420` |
| `top_bottom_422.avif` | 32×32 | red top, blue bottom; 4:2:2 | `-y 422` |
| `clap.avif` | 32×32 shown | green, red, blue columns (16, 32, 16 px) cropped by `clap` to the red middle | `-y 444 --crop 16,0,32,32` |

The full script is in `docs/superpowers/plans/2026-09-24-photon-avif.md`, Task 1.
````

- [ ] **Step 5: Add the fixture loader to `testutil.rs`**

Append to `crates/photon-core/src/testutil.rs`:

```rust
/// One of the `avifenc`-made files in `testdata/avif` (see its README for what each holds).
pub fn avif_fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/avif")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}
```

(`testutil.rs` already imports `std::path::{Path, PathBuf}`.)

- [ ] **Step 6: Write the failing tests for the AV1 wrapper**

Create `crates/photon-core/src/avif/mod.rs` with only:

```rust
//! AVIF, decoded in pure Rust: `zenavif-parse` for the container, `rav1d` for the AV1
//! inside it. `image` decodes AVIF only through dav1d, a C library, and photon takes no
//! native dependencies (`docs/superpowers/specs/2026-09-24-photon-avif-design.md`).

mod av1;
```

Add `pub mod avif;` to `crates/photon-core/src/lib.rs`, in alphabetical order (before
`pub mod decode;`).

Create `crates/photon-core/src/avif/av1.rs` holding the `Planes` struct and a `decode` that
returns `Err("unimplemented".into())`, plus these tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::avif_fixture;

    /// The AV1 payload of a single-image fixture.
    fn primary(name: &str) -> Vec<u8> {
        let bytes = avif_fixture(name);
        let parser = zenavif_parse::AvifParser::from_bytes(&bytes).unwrap();
        parser.primary_data().unwrap().into_owned()
    }

    #[test]
    fn decodes_an_item_to_its_planes() {
        let p = decode(&primary("red_blue_709_limited.avif")).unwrap();
        assert_eq!((p.width, p.height, p.depth), (64, 32, 8));
        assert_eq!((p.mono, p.shift), (false, (1, 1)));
        assert_eq!((p.chroma_width(), p.chroma_height()), (32, 16));
        assert_eq!((p.y.len(), p.u.len(), p.v.len()), (64 * 32, 32 * 16, 32 * 16));
        // The sequence header's own colour description: BT.709, limited range.
        assert_eq!((p.matrix, p.full_range), (1, false));
    }

    #[test]
    fn reports_each_chroma_layout() {
        let layout = |name| {
            let p = decode(&primary(name)).unwrap();
            (p.mono, p.shift, p.u.len())
        };
        assert_eq!(layout("red_blue_444_full.avif"), (false, (0, 0), 64 * 32));
        assert_eq!(layout("top_bottom_422.avif"), (false, (1, 0), 16 * 32));
        assert_eq!(layout("mono.avif"), (true, (0, 0), 0));
    }

    /// 33×17 at 4:2:0 has a 17×9 chroma plane: rounded up, not down.
    #[test]
    fn an_odd_size_rounds_the_chroma_plane_up() {
        let p = decode(&primary("odd_420.avif")).unwrap();
        assert_eq!((p.width, p.height), (33, 17));
        assert_eq!((p.chroma_width(), p.chroma_height()), (17, 9));
        assert_eq!(p.u.len(), 17 * 9);
    }

    /// Ten-bit samples arrive whole. Read as bytes, white would top out at 255 rather than
    /// near 1023.
    #[test]
    fn keeps_samples_deeper_than_eight_bits() {
        let bytes = avif_fixture("grid_10bit.avif");
        let parser = zenavif_parse::AvifParser::from_bytes(&bytes).unwrap();
        // Tile 3 is the white quadrant.
        let p = decode(&parser.tile_data(3).unwrap()).unwrap();
        assert_eq!(p.depth, 10);
        assert!(p.y.iter().copied().max().unwrap() > 900, "white luma should be near 1023");
    }

    #[test]
    fn refuses_garbage_and_empty_items() {
        assert!(decode(&[]).is_err());
        assert!(decode(b"definitely not AV1 at all").is_err());
        let mut cut = primary("red_blue_709_limited.avif");
        cut.truncate(cut.len() / 2);
        assert!(decode(&cut).is_err());
    }
}
```

- [ ] **Step 7: Run the tests to see them fail**

Run: `cargo test -p photon-core --lib avif::av1`
Expected: the four decoding tests FAIL on `unwrap()` of `Err("unimplemented")`.
`refuses_garbage_and_empty_items` passes against the stub; that is expected, and Step 10's
probe is what proves it.

- [ ] **Step 8: Implement the wrapper**

Replace the stub `decode` in `crates/photon-core/src/avif/av1.rs` with the full module body
(the tests stay below it):

```rust
//! The only `unsafe` code in photon: a wrapper over the dav1d-style API that `rav1d`'s
//! crates.io release exports, which is the only way into it. Each resource it hands out is
//! owned by a guard whose `Drop` gives it back, so no early return can leak one.

use rav1d::include::dav1d::data::Dav1dData;
use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
use rav1d::include::dav1d::headers::{
    DAV1D_PIXEL_LAYOUT_I400, DAV1D_PIXEL_LAYOUT_I420, DAV1D_PIXEL_LAYOUT_I422,
    DAV1D_PIXEL_LAYOUT_I444,
};
use rav1d::include::dav1d::picture::Dav1dPicture;
use rav1d::src::lib::{
    dav1d_close, dav1d_data_create, dav1d_data_unref, dav1d_default_settings, dav1d_get_picture,
    dav1d_open, dav1d_picture_unref, dav1d_send_data,
};
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr::NonNull;

/// The largest frame rav1d is allowed to allocate, in pixels: `image`'s 512 MiB decode
/// bound at four bytes a pixel, so a header claiming absurd dimensions is refused up front.
const MAX_FRAME_PIXELS: u32 = 512 * 1024 * 1024 / 4;

/// dav1d reports "not now" as `-EAGAIN`, and `EAGAIN` is 11 on Linux and Windows but 35 on
/// macOS: it has to come from the platform's own headers.
const AGAIN: i32 = -libc::EAGAIN;

/// One decoded AV1 frame. Every plane is widened to 16-bit samples whatever the bit depth,
/// and packed row after row with no stride padding.
pub(crate) struct Planes {
    pub width: usize,
    pub height: usize,
    /// 8, 10 or 12.
    pub depth: u8,
    /// No chroma planes: `u` and `v` are empty.
    pub mono: bool,
    /// Chroma subsampling as (x, y) shifts: (1, 1) is 4:2:0, (1, 0) 4:2:2, (0, 0) 4:4:4.
    pub shift: (u32, u32),
    pub y: Vec<u16>,
    pub u: Vec<u16>,
    pub v: Vec<u16>,
    /// H.273 matrix coefficients and range, as the AV1 sequence header states them.
    pub matrix: u16,
    pub full_range: bool,
}

impl Planes {
    pub fn chroma_width(&self) -> usize {
        (self.width + (1 << self.shift.0) - 1) >> self.shift.0
    }

    pub fn chroma_height(&self) -> usize {
        (self.height + (1 << self.shift.1) - 1) >> self.shift.1
    }
}

struct Context(Option<Dav1dContext>);

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: `self.0` is either `None`, which `dav1d_close` ignores, or the context
        // `dav1d_open` wrote there, which nothing else holds.
        unsafe { dav1d_close(NonNull::new(&mut self.0)) }
    }
}

struct Data(Dav1dData);

impl Drop for Data {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from `dav1d_data_create`, or is what `dav1d_send_data` left
        // of it; unreferencing an emptied buffer is a no-op.
        unsafe { dav1d_data_unref(NonNull::new(&mut self.0)) }
    }
}

struct Picture(Dav1dPicture);

impl Drop for Picture {
    fn drop(&mut self) {
        // SAFETY: a `Picture` is only built from a picture `dav1d_get_picture` returned.
        unsafe { dav1d_picture_unref(NonNull::new(&mut self.0)) }
    }
}

/// Decodes one AV1 item - a still image, a grid tile or an alpha plane - to its planes.
pub(crate) fn decode(obu: &[u8]) -> Result<Planes, String> {
    if obu.is_empty() {
        return Err("empty AV1 item".into());
    }
    let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
    // SAFETY: `dav1d_default_settings` writes every field of the struct it is pointed at.
    let mut settings = unsafe {
        dav1d_default_settings(NonNull::new_unchecked(settings.as_mut_ptr()));
        settings.assume_init()
    };
    // One thread: the thumbnail pool already decodes `MAX_WORKERS` photos at once, and
    // rav1d's default of a thread per core under each of them would oversubscribe the machine.
    settings.n_threads = 1;
    settings.max_frame_delay = 1;
    settings.frame_size_limit = MAX_FRAME_PIXELS;

    let mut context = Context(None);
    // SAFETY: both pointers are to live locals; on success the new context is owned by
    // `context`, whose `Drop` closes it.
    let opened = unsafe { dav1d_open(NonNull::new(&mut context.0), NonNull::new(&mut settings)) };
    if opened.0 != 0 {
        return Err(format!("could not start the AV1 decoder ({})", opened.0));
    }

    // SAFETY: all-zero is a valid, empty `Dav1dData`: its pointers are `Option`s and the
    // rest are plain integers.
    let mut data = Data(unsafe { std::mem::zeroed() });
    // SAFETY: `data_create` initialises `data` and returns a buffer of `obu.len()` bytes
    // that `data` owns, or null when it cannot allocate.
    let buffer = unsafe { dav1d_data_create(NonNull::new(&mut data.0), obu.len()) };
    if buffer.is_null() {
        return Err("out of memory for the AV1 item".into());
    }
    // SAFETY: `buffer` is a fresh allocation of exactly `obu.len()` bytes that nothing else
    // can reach yet, and `obu` cannot overlap it.
    unsafe { std::ptr::copy_nonoverlapping(obu.as_ptr(), buffer, obu.len()) };

    let picture = loop {
        if data.0.sz > 0 {
            // SAFETY: `context.0` is the open decoder and `data` a live buffer from
            // `data_create`; `send_data` takes what it consumes and shrinks `sz`.
            let sent = unsafe { dav1d_send_data(context.0, NonNull::new(&mut data.0)) };
            if sent.0 != 0 && sent.0 != AGAIN {
                return Err(format!("the AV1 decoder refused the item ({})", sent.0));
            }
        }
        // SAFETY: all-zero is a valid empty `Dav1dPicture`, which `get_picture` overwrites.
        let mut out: Dav1dPicture = unsafe { std::mem::zeroed() };
        // SAFETY: `context.0` is the open decoder and `out` a live local.
        let got = unsafe { dav1d_get_picture(context.0, NonNull::new(&mut out)) };
        if got.0 == 0 {
            break Picture(out);
        }
        // "Not now" with input still to send means send it; with nothing left, the item
        // held no frame at all. Without the second half this loop would spin forever on a
        // truncated item.
        if got.0 != AGAIN || data.0.sz == 0 {
            return Err(format!("the AV1 item decoded to no picture ({})", got.0));
        }
    };
    planes(&picture.0)
}

fn planes(picture: &Dav1dPicture) -> Result<Planes, String> {
    let p = &picture.p;
    let (width, height, depth) = (p.w as usize, p.h as usize, p.bpc as u8);
    let (mono, shift) = match p.layout {
        DAV1D_PIXEL_LAYOUT_I400 => (true, (0, 0)),
        DAV1D_PIXEL_LAYOUT_I420 => (false, (1, 1)),
        DAV1D_PIXEL_LAYOUT_I422 => (false, (1, 0)),
        DAV1D_PIXEL_LAYOUT_I444 => (false, (0, 0)),
        other => return Err(format!("unknown AV1 pixel layout {other}")),
    };
    if !matches!(depth, 8 | 10 | 12) || width == 0 || height == 0 {
        return Err(format!("unusable AV1 picture: {width}x{height} at {depth} bits"));
    }
    let header = picture.seq_hdr.ok_or("AV1 picture without a sequence header")?;
    // SAFETY: a returned picture holds a reference to its sequence header, alive until the
    // picture is unreferenced, which cannot happen while `picture` is borrowed here.
    let header = unsafe { header.as_ref() };
    let mut planes = Planes {
        width,
        height,
        depth,
        mono,
        shift,
        y: Vec::new(),
        u: Vec::new(),
        v: Vec::new(),
        matrix: header.mtrx as u16,
        full_range: header.color_range != 0,
    };
    planes.y = copy_plane(picture.data[0], picture.stride[0], width, height, depth)?;
    if !mono {
        let (cw, ch) = (planes.chroma_width(), planes.chroma_height());
        planes.u = copy_plane(picture.data[1], picture.stride[1], cw, ch, depth)?;
        planes.v = copy_plane(picture.data[2], picture.stride[1], cw, ch, depth)?;
    }
    Ok(planes)
}

fn copy_plane(
    data: Option<NonNull<c_void>>,
    stride: isize,
    width: usize,
    height: usize,
    depth: u8,
) -> Result<Vec<u16>, String> {
    let base = data.ok_or("AV1 picture missing a plane")?.as_ptr() as *const u8;
    let mut out = Vec::with_capacity(width * height);
    for row in 0..height {
        // SAFETY: dav1d lays a plane out as `height` rows `stride` bytes apart, each holding
        // `width` samples of one byte at 8 bits and two (aligned) above, and the picture
        // outlives this copy.
        unsafe {
            let start = base.offset(row as isize * stride);
            if depth == 8 {
                let samples = std::slice::from_raw_parts(start, width);
                out.extend(samples.iter().map(|&s| u16::from(s)));
            } else {
                let samples = std::slice::from_raw_parts(start as *const u16, width);
                out.extend_from_slice(samples);
            }
        }
    }
    Ok(out)
}
```

- [ ] **Step 9: Run the tests to see them pass**

Run: `cargo test -p photon-core --lib avif::av1`
Expected: 5 passed. (Until Task 3 uses them, `clippy` may flag `Planes` fields and
`decode` as dead code. Put `#![allow(dead_code)]` at the top of `avif/mod.rs` for now, with the
comment `// Until decode_avif (Task 3) uses the wrapper.`, and remove it in Task 3.)

- [ ] **Step 10: Revert probes**

Make each change, run `cargo test -p photon-core --lib avif::av1`, confirm the named test
FAILS, then undo it exactly:
- In `copy_plane`, replace the whole `if depth == 8 { … } else { … }` with only the 8-bit
  branch → `keeps_samples_deeper_than_eight_bits` fails.
- Change `DAV1D_PIXEL_LAYOUT_I422 => (false, (1, 0))` to `(false, (1, 1))` →
  `reports_each_chroma_layout` fails.
- Change `chroma_width`'s `+ (1 << self.shift.0) - 1` to `+ 0` →
  `an_odd_size_rounds_the_chroma_plane_up` fails.
- Change `if got.0 != AGAIN || data.0.sz == 0` to `if got.0 != AGAIN` → the truncated case
  in `refuses_garbage_and_empty_items` must now **hang or fail**. Run it with
  `timeout 60 cargo test -p photon-core --lib avif::av1::tests::refuses_garbage_and_empty_items`.
  If it still passes, rav1d reports the truncation as something other than EAGAIN. Write that
  down in the commit message as a finding, and keep the guard: it costs nothing and bounds
  the loop.

- [ ] **Step 11: Rust gate, then commit**

Run the five gate commands from Global Constraints. Then:

```bash
git add Cargo.toml Cargo.lock crates/photon-core/Cargo.toml crates/photon-core/testdata crates/photon-core/src/avif crates/photon-core/src/lib.rs crates/photon-core/src/testutil.rs
git commit -m "feat(avif): decode AV1 items with rav1d, fixtures made with avifenc

<one paragraph: why rav1d without assembly, why the unsafe is confined to av1.rs, and the
probe results from step 10>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: YUV to RGB

**Files:**
- Create: `crates/photon-core/src/avif/yuv.rs`
- Modify: `crates/photon-core/src/avif/mod.rs` (add `mod yuv;`)

**Interfaces:**
- Consumes: `av1::Planes` (Task 1).
- Produces: `pub(super) fn yuv::to_rgb(p: &Planes, matrix: u16, full_range: bool) -> RgbImage`;
  `pub(super) fn yuv::attach_alpha(rgb: RgbImage, alpha: &Planes, premultiplied: bool) -> Result<RgbaImage, String>`;
  `pub(super) const yuv::IDENTITY: u16 = 0`.

These tests build `Planes` by hand, with no AV1 involved, so each rule is pinned by a known
sample value.

- [ ] **Step 1: Write the failing tests**

Create `crates/photon-core/src/avif/yuv.rs` with stub bodies (`to_rgb` returning
`RgbImage::new(1, 1)`, `attach_alpha` returning `Err(String::new())`) and these tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// A width×height picture of one YUV colour, at `depth` bits and `shift` subsampling.
    fn flat(width: usize, height: usize, depth: u8, shift: (u32, u32), yuv: [u16; 3]) -> Planes {
        let mut p = Planes {
            width,
            height,
            depth,
            mono: false,
            shift,
            y: vec![yuv[0]; width * height],
            u: Vec::new(),
            v: Vec::new(),
            matrix: 2,
            full_range: true,
        };
        let chroma = p.chroma_width() * p.chroma_height();
        p.u = vec![yuv[1]; chroma];
        p.v = vec![yuv[2]; chroma];
        p
    }

    fn close(got: [u8; 3], want: [u8; 3]) -> bool {
        got.iter().zip(want).all(|(&g, w)| (i32::from(g) - i32::from(w)).abs() <= 2)
    }

    /// Pure red coded with BT.709's weights, limited range: Y 63, Cb 102, Cr 240.
    const RED_709_LIMITED: [u16; 3] = [63, 102, 240];

    #[test]
    fn converts_bt709_limited_range() {
        let rgb = to_rgb(&flat(2, 2, 8, (1, 1), RED_709_LIMITED), 1, false);
        let px = rgb.get_pixel(1, 1).0;
        assert!(close(px, [255, 0, 0]), "{px:?}");
    }

    /// The same samples read with BT.601's weights come out visibly wrong: this is what a
    /// matrix mix-up looks like, and why the matrix is read from the file.
    #[test]
    fn the_matrix_changes_the_colour() {
        let rgb = to_rgb(&flat(2, 2, 8, (1, 1), RED_709_LIMITED), 6, false);
        let [r, g, _] = rgb.get_pixel(0, 0).0;
        assert!(r < 245 || g > 10, "BT.601 should not reproduce BT.709 red: {r} {g}");
    }

    #[test]
    fn full_range_uses_the_whole_scale() {
        // Full-range black and white are 0 and 255, where limited range has 16 and 235.
        let white = to_rgb(&flat(1, 1, 8, (0, 0), [255, 128, 128]), 6, true);
        assert_eq!(white.get_pixel(0, 0).0, [255, 255, 255]);
        let limited_white = to_rgb(&flat(1, 1, 8, (0, 0), [235, 128, 128]), 6, false);
        assert_eq!(limited_white.get_pixel(0, 0).0, [255, 255, 255]);
        let read_as_full = to_rgb(&flat(1, 1, 8, (0, 0), [235, 128, 128]), 6, true);
        assert!(read_as_full.get_pixel(0, 0).0[0] < 240);
    }

    #[test]
    fn deeper_samples_scale_down_to_eight_bits() {
        let white = to_rgb(&flat(1, 1, 10, (0, 0), [1023, 512, 512]), 6, true);
        assert_eq!(white.get_pixel(0, 0).0, [255, 255, 255]);
        let grey = to_rgb(&flat(1, 1, 12, (0, 0), [2048, 2048, 2048]), 6, true);
        assert!(close(grey.get_pixel(0, 0).0, [128, 128, 128]));
    }

    /// Identity (matrix 0) stores G in the luma plane, B in U and R in V.
    #[test]
    fn identity_is_gbr() {
        let rgb = to_rgb(&flat(1, 1, 8, (0, 0), [0, 255, 128]), IDENTITY, true);
        assert_eq!(rgb.get_pixel(0, 0).0, [128, 0, 255]);
    }

    #[test]
    fn monochrome_is_grey() {
        let mut p = flat(2, 1, 8, (0, 0), [200, 0, 0]);
        p.mono = true;
        p.u.clear();
        p.v.clear();
        assert_eq!(to_rgb(&p, 6, true).get_pixel(1, 0).0, [200, 200, 200]);
    }

    /// Each pixel takes the chroma sample covering it: at 4:2:0 on an odd width the last
    /// column reads the rounded-up chroma column, and nothing indexes past the plane.
    #[test]
    fn subsampled_chroma_is_read_per_pixel() {
        let mut p = flat(3, 3, 8, (1, 1), [128, 128, 128]);
        // Chroma is 2×2; make its bottom-right sample red-ish.
        p.v[3] = 255;
        let rgb = to_rgb(&p, 6, true);
        assert!(rgb.get_pixel(2, 2).0[0] > 200, "(2,2) reads chroma (1,1)");
        assert!(rgb.get_pixel(1, 1).0[0] < 160, "(1,1) reads chroma (0,0)");
    }

    #[test]
    fn attaches_alpha_from_its_luma() {
        let rgb = RgbImage::from_pixel(2, 1, Rgb([200, 100, 0]));
        let mut alpha = flat(2, 1, 8, (0, 0), [0, 0, 0]);
        alpha.mono = true;
        alpha.y = vec![128, 255];
        let rgba = attach_alpha(rgb.clone(), &alpha, false).unwrap();
        assert_eq!(rgba.get_pixel(0, 0).0, [200, 100, 0, 128]);
        assert_eq!(rgba.get_pixel(1, 0).0, [200, 100, 0, 255]);
        // Premultiplied colour is divided back out: 100 at alpha 128 was 200 at full.
        let pre = RgbImage::from_pixel(2, 1, Rgb([100, 50, 0]));
        let rgba = attach_alpha(pre, &alpha, true).unwrap();
        assert!(close([rgba.get_pixel(0, 0).0[0], rgba.get_pixel(0, 0).0[1], 0], [199, 100, 0]));
        // A mismatched alpha plane is an error, not a panic.
        let mut small = alpha;
        small.width = 1;
        small.y.truncate(1);
        assert!(attach_alpha(rgb, &small, false).is_err());
    }
}
```

Add `mod yuv;` below `mod av1;` in `avif/mod.rs`.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p photon-core --lib avif::yuv`
Expected: every test FAILS (the stub picture is 1×1 black, so `get_pixel` panics out of
bounds or the colours are wrong, and `attach_alpha` errors).

- [ ] **Step 3: Implement**

Replace the stubs with:

```rust
//! YUV to RGB for decoded AV1 planes. Nearest chroma sample, no upsampling filter: this
//! feeds thumbnails and edits, and the viewer shows the webview's own decode of the file.

use super::av1::Planes;
use image::{Rgb, RgbImage, Rgba, RgbaImage};

/// H.273 matrix coefficients 0: the planes are G, B, R rather than luma and chroma.
pub(super) const IDENTITY: u16 = 0;

/// Luma weights (Kr, Kb) for an H.273 matrix. Unspecified (2), and every matrix photon does
/// not know, falls back to BT.601 - what libavif assumes in the same case.
fn weights(matrix: u16) -> (f32, f32) {
    match matrix {
        1 => (0.2126, 0.0722),
        // BT.2020 constant-luminance (10) is treated as its non-constant sibling: close
        // enough for a thumbnail, and no camera writes it.
        9 | 10 => (0.2627, 0.0593),
        _ => (0.299, 0.114),
    }
}

/// Maps samples of one bit depth and range onto 0..1 (luma) and -0.5..0.5 (chroma).
#[derive(Clone, Copy)]
struct Levels {
    max: f32,
    scale: f32,
    full: bool,
}

impl Levels {
    fn new(depth: u8, full: bool) -> Self {
        Self {
            max: ((1u32 << depth) - 1) as f32,
            scale: (1u32 << (depth - 8)) as f32,
            full,
        }
    }

    fn luma(self, s: u16) -> f32 {
        if self.full {
            f32::from(s) / self.max
        } else {
            (f32::from(s) - 16.0 * self.scale) / (219.0 * self.scale)
        }
    }

    fn chroma(self, s: u16) -> f32 {
        if self.full {
            (f32::from(s) - (self.max + 1.0) / 2.0) / self.max
        } else {
            (f32::from(s) - 128.0 * self.scale) / (224.0 * self.scale)
        }
    }
}

fn to8(c: f32) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub(super) fn to_rgb(p: &Planes, matrix: u16, full_range: bool) -> RgbImage {
    let levels = Levels::new(p.depth, full_range);
    let (kr, kb) = weights(matrix);
    let kg = 1.0 - kr - kb;
    let chroma_width = p.chroma_width();
    RgbImage::from_fn(p.width as u32, p.height as u32, |x, y| {
        let (x, y) = (x as usize, y as usize);
        let luma = levels.luma(p.y[y * p.width + x]);
        if p.mono {
            let g = to8(luma);
            return Rgb([g, g, g]);
        }
        let i = (y >> p.shift.1) * chroma_width + (x >> p.shift.0);
        let (u, v) = (p.u[i], p.v[i]);
        let [r, g, b] = if matrix == IDENTITY {
            [levels.luma(v), luma, levels.luma(u)]
        } else {
            let (cb, cr) = (levels.chroma(u), levels.chroma(v));
            let r = luma + 2.0 * (1.0 - kr) * cr;
            let b = luma + 2.0 * (1.0 - kb) * cb;
            [r, (luma - kr * r - kb * b) / kg, b]
        };
        Rgb([to8(r), to8(g), to8(b)])
    })
}

/// Joins the colour picture with its alpha item's luma, dividing premultiplied colour back
/// out so the result is straight alpha like every other RGBA image `image` hands around.
pub(super) fn attach_alpha(
    rgb: RgbImage,
    alpha: &Planes,
    premultiplied: bool,
) -> Result<RgbaImage, String> {
    if (alpha.width, alpha.height) != (rgb.width() as usize, rgb.height() as usize) {
        return Err("the alpha plane does not match the picture's size".into());
    }
    let levels = Levels::new(alpha.depth, alpha.full_range);
    Ok(RgbaImage::from_fn(rgb.width(), rgb.height(), |x, y| {
        let a = to8(levels.luma(alpha.y[y as usize * alpha.width + x as usize]));
        let straight = |c: u8| {
            if premultiplied && a > 0 {
                ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8
            } else {
                c
            }
        };
        let [r, g, b] = rgb.get_pixel(x, y).0;
        Rgba([straight(r), straight(g), straight(b), a])
    }))
}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p photon-core --lib avif::yuv`
Expected: 8 passed.

- [ ] **Step 5: Revert probes**

Make each change, confirm the named test FAILS, then undo it exactly:
- In `weights`, change `1 => (0.2126, 0.0722)` to `1 => (0.299, 0.114)` →
  `converts_bt709_limited_range` fails.
- In `Levels::luma`, make both branches the full-range one →
  `converts_bt709_limited_range` and `full_range_uses_the_whole_scale` fail.
- Swap `[levels.luma(v), luma, levels.luma(u)]` to `[levels.luma(u), luma, levels.luma(v)]`
  → `identity_is_gbr` fails.
- In `attach_alpha`, make `straight` return `c` always → `attaches_alpha_from_its_luma` fails.
- Change `(y >> p.shift.1)` to `(y >> p.shift.0)` in `to_rgb`. This one does **not** fail
  here: every shift in these tests is symmetric. It is pinned in Task 3 by
  `top_bottom_422.avif`. Note it and move on.

- [ ] **Step 6: Rust gate, then commit**

Run the gate. Then:

```bash
git add crates/photon-core/src/avif
git commit -m "feat(avif): convert decoded planes to RGB, and attach alpha

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Decoding an AVIF file: sniffing, grid, transforms, dimensions

**Files:**
- Modify: `crates/photon-core/src/avif/mod.rs`

**Interfaces:**
- Consumes: `av1::decode`, `av1::Planes`, `yuv::to_rgb`, `yuv::attach_alpha`.
- Produces: `pub(crate) fn avif::is_avif(head: &[u8]) -> bool`;
  `pub fn avif::decode_avif(bytes: &[u8]) -> image::ImageResult<DynamicImage>`;
  `pub fn avif::avif_dimensions(bytes: &[u8]) -> Option<(u32, u32)>`.
  Decode errors are `ImageError::Decoding` with hint `Exact(ImageFormat::Avif)`.

- [ ] **Step 1: Write the failing tests**

Give `avif/mod.rs` stub versions of the three public functions and append the tests:

```rust
use image::{DynamicImage, ImageError, ImageResult};

pub(crate) fn is_avif(_head: &[u8]) -> bool {
    false
}

pub fn decode_avif(_bytes: &[u8]) -> ImageResult<DynamicImage> {
    Err(ImageError::IoError(std::io::Error::other("unimplemented")))
}

pub fn avif_dimensions(_bytes: &[u8]) -> Option<(u32, u32)> {
    None
}
```



```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::avif_fixture;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const BLACK: [u8; 4] = [0, 0, 0, 255];

    /// Decodes `name` and checks its size (both as decoded and as `avif_dimensions`
    /// reports it) and the colour at each point, within ±12 of the source colour: lossy
    /// AV1 moves flat blue to about 243. A matrix mix-up moves red by 22.
    fn check(name: &str, size: (u32, u32), points: &[((u32, u32), [u8; 4])]) {
        let bytes = avif_fixture(name);
        let img = decode_avif(&bytes).unwrap_or_else(|e| panic!("{name}: {e}")).to_rgba8();
        assert_eq!(img.dimensions(), size, "{name}: decoded size");
        assert_eq!(avif_dimensions(&bytes), Some(size), "{name}: avif_dimensions");
        for &((x, y), want) in points {
            let got = img.get_pixel(x, y).0;
            let near = got.iter().zip(want).all(|(&g, w)| (i32::from(g) - i32::from(w)).abs() <= 12);
            assert!(near, "{name} at ({x},{y}): got {got:?}, want {want:?}");
        }
    }

    #[test]
    fn decodes_bt709_limited_range_from_the_colr_box() {
        check("red_blue_709_limited.avif", (64, 32), &[((16, 16), RED), ((48, 16), BLUE)]);
    }

    /// `colr` is an ICC profile here, so the matrix and range come from the AV1 sequence
    /// header. Defaulting to BT.601 full range instead turns red to about 233.
    #[test]
    fn falls_back_to_the_sequence_header_colour() {
        check("icc_709_limited.avif", (64, 32), &[((16, 16), RED), ((48, 16), BLUE)]);
    }

    #[test]
    fn decodes_full_range_444() {
        check("red_blue_444_full.avif", (64, 32), &[((16, 16), RED), ((48, 16), BLUE)]);
    }

    /// Tiles are laid out row by row: red, green / blue, white.
    #[test]
    fn stitches_a_ten_bit_grid() {
        check(
            "grid_10bit.avif",
            (128, 128),
            &[((32, 32), RED), ((96, 32), GREEN), ((32, 96), BLUE), ((96, 96), WHITE)],
        );
    }

    /// `irot` 1 is 90° anticlockwise: red|blue becomes blue over red.
    #[test]
    fn applies_irot() {
        check("irot90.avif", (32, 64), &[((16, 8), BLUE), ((16, 56), RED)]);
    }

    /// `imir` 1 mirrors left-to-right, as libavif and the browsers read it.
    #[test]
    fn applies_imir() {
        check("imir.avif", (64, 32), &[((8, 16), BLUE), ((56, 16), RED)]);
    }

    #[test]
    fn applies_clap() {
        check("clap.avif", (32, 32), &[((2, 16), RED), ((29, 16), RED)]);
    }

    #[test]
    fn keeps_alpha() {
        check("alpha.avif", (32, 32), &[((8, 16), [255, 0, 0, 128]), ((24, 16), RED)]);
    }

    #[test]
    fn decodes_monochrome() {
        check("mono.avif", (64, 32), &[((16, 16), BLACK), ((48, 16), WHITE)]);
    }

    #[test]
    fn decodes_an_odd_sized_420_picture() {
        check("odd_420.avif", (33, 17), &[((0, 0), RED), ((32, 16), RED)]);
    }

    /// 4:2:2 halves chroma horizontally only. Halving it vertically as well would read the
    /// blue bottom half's chroma from the red top half's rows.
    #[test]
    fn subsamples_422_horizontally_only() {
        check("top_bottom_422.avif", (32, 32), &[((16, 4), RED), ((16, 28), BLUE)]);
    }

    /// A generic `mif1` major brand with `avif` among the compatible ones is an AVIF.
    /// zenavif-parse alone refuses it.
    #[test]
    fn accepts_a_mif1_major_brand() {
        let mut bytes = avif_fixture("red_blue_709_limited.avif");
        assert_eq!(&bytes[8..12], b"avif");
        bytes[8..12].copy_from_slice(b"mif1");
        assert!(is_avif(&bytes));
        let img = decode_avif(&bytes).unwrap();
        assert_eq!((img.width(), img.height()), (64, 32));
        assert_eq!(avif_dimensions(&bytes), Some((64, 32)));
    }

    #[test]
    fn sniffs_only_avif_brands() {
        assert!(is_avif(&avif_fixture("mono.avif")));
        // HEIC is the same container with other brands: not ours.
        let mut heic = avif_fixture("mono.avif");
        heic[8..12].copy_from_slice(b"heic");
        let size = u32::from_be_bytes([heic[0], heic[1], heic[2], heic[3]]) as usize;
        for brand in heic[16..size].chunks_mut(4) {
            if brand == b"avif" {
                brand.copy_from_slice(b"heic");
            }
        }
        assert!(!is_avif(&heic));
        assert!(!is_avif(b"\xFF\xD8\xFF\xE0 a jpeg"));
        assert!(!is_avif(b""));
        assert!(!is_avif(b"\0\0\0\x08ftypavif"), "a box too small to hold its brand");
    }

    #[test]
    fn a_truncated_avif_is_an_error_not_a_panic() {
        let bytes = avif_fixture("red_blue_709_limited.avif");
        for cut in [16, bytes.len() / 2, bytes.len() - 1] {
            let err = decode_avif(&bytes[..cut]).unwrap_err();
            assert!(matches!(err, ImageError::Decoding(_)), "cut at {cut}: {err:?}");
            assert_eq!(avif_dimensions(&bytes[..cut]), None, "cut at {cut}");
        }
    }
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p photon-core --lib avif::tests`
Expected: every test FAILS (stubs).

- [ ] **Step 3: Implement**

Replace the body of `avif/mod.rs` (keep the module doc comment, `mod av1; mod yuv;` and the
tests; remove Task 1's `#![allow(dead_code)]`) with:

```rust
use std::borrow::Cow;

use image::error::{DecodingError, ImageFormatHint};
use image::{DynamicImage, ImageError, ImageFormat, ImageResult, RgbImage, imageops};
use zenavif_parse::{
    AV1Metadata, AvifParser, ColorInformation, DecodeConfig, GridConfig, Unstoppable,
};

/// The bound `image`'s default limits put on every other format's decode, which
/// `decode::decode_oriented`'s memory reasoning relies on.
const MAX_DECODE_BYTES: u64 = 512 * 1024 * 1024;

/// Whether `head`, the first bytes of a file, is an AVIF: an `ftyp` box whose major *or
/// compatible* brands include `avif` (a still) or `avis` (a sequence). The compatible list
/// matters: a file whose major brand is the generic `mif1` is still an AVIF.
pub(crate) fn is_avif(head: &[u8]) -> bool {
    brands(head).is_some_and(|(major, compatible)| {
        is_avif_brand(&major) || compatible.iter().any(is_avif_brand)
    })
}

/// The major and compatible brands of the `ftyp` box `head` starts with, if it starts with
/// one. Sizes 0 ("to the end") and 1 (64-bit) are legal but no still-image writer uses them
/// for an `ftyp`; anything under 16 cannot hold a major brand and a version.
fn brands(head: &[u8]) -> Option<([u8; 4], &[[u8; 4]])> {
    if head.len() < 16 || &head[4..8] != b"ftyp" {
        return None;
    }
    let size = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
    if size < 16 {
        return None;
    }
    let major = [head[8], head[9], head[10], head[11]];
    let compatible = head.get(16..size.min(head.len())).unwrap_or(&[]);
    Some((major, compatible.as_chunks::<4>().0))
}

fn is_avif_brand(brand: &[u8; 4]) -> bool {
    matches!(brand, b"avif" | b"avis")
}

/// Decodes an AVIF to the picture as it is meant to be shown: grid tiles stitched, alpha
/// attached, and the container's crop, rotation and mirror applied.
pub fn decode_avif(bytes: &[u8]) -> ImageResult<DynamicImage> {
    let bytes = with_avif_major(bytes);
    let parser = parse(&bytes)?;
    let img = if parser.grid_tile_count() > 0 {
        // A grid's alpha would be a second grid; photon shows such a photo opaque.
        DynamicImage::ImageRgb8(grid(&parser)?)
    } else {
        let planes = av1::decode(&parser.primary_data().map_err(parse_failed)?).map_err(failed)?;
        let (matrix, full_range) = colour(&parser, &planes);
        let rgb = yuv::to_rgb(&planes, matrix, full_range);
        match parser.alpha_data() {
            None => DynamicImage::ImageRgb8(rgb),
            Some(alpha) => {
                let alpha = av1::decode(&alpha.map_err(parse_failed)?).map_err(failed)?;
                let rgba =
                    yuv::attach_alpha(rgb, &alpha, parser.premultiplied_alpha()).map_err(failed)?;
                DynamicImage::ImageRgba8(rgba)
            }
        }
    };
    Ok(transform(&parser, img))
}

/// The dimensions `decode_avif` would return, read from the container and the AV1 sequence
/// headers alone - no pixel is decoded.
pub fn avif_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let bytes = with_avif_major(bytes);
    let parser = parse(&bytes).ok()?;
    let (w, h) = if parser.grid_tile_count() > 0 {
        // A grid item's own payload is the grid description, not AV1: the size comes from
        // the layout, or from a tile when the layout leaves it at zero.
        let tile = AV1Metadata::parse_av1_bitstream(&parser.tile_data(0).ok()?).ok()?;
        grid_size(
            parser.grid_config()?,
            (tile.max_frame_width.get(), tile.max_frame_height.get()),
        )
    } else {
        let meta = parser.primary_metadata().ok()?;
        (meta.max_frame_width.get(), meta.max_frame_height.get())
    };
    let (w, h) = clean_aperture(&parser, w, h).map_or((w, h), |(_, _, cw, ch)| (cw, ch));
    Some(match parser.rotation().map(|r| r.angle) {
        Some(90 | 270) => (h, w),
        _ => (w, h),
    })
}

/// `bytes`, or a copy whose major brand is the `avif`/`avis` it lists as compatible.
/// zenavif-parse refuses any other major brand, even leniently, while libavif and the
/// browsers accept a generic `mif1` file that declares AVIF compatibility. Only this copy
/// is changed - never the file.
fn with_avif_major(bytes: &[u8]) -> Cow<'_, [u8]> {
    let Some((major, compatible)) = brands(bytes) else {
        return Cow::Borrowed(bytes);
    };
    match compatible.iter().find(|brand| is_avif_brand(brand)) {
        Some(brand) if !is_avif_brand(&major) => {
            let mut copy = bytes.to_vec();
            copy[8..12].copy_from_slice(brand);
            Cow::Owned(copy)
        }
        _ => Cow::Borrowed(bytes),
    }
}

fn parse(bytes: &[u8]) -> ImageResult<AvifParser<'_>> {
    let config = DecodeConfig::default()
        .with_peak_memory_limit(MAX_DECODE_BYTES)
        .with_total_megapixels_limit((MAX_DECODE_BYTES / 4 / 1_000_000) as u32);
    AvifParser::from_bytes_with_config(bytes, &config, &Unstoppable).map_err(parse_failed)
}

/// The container's `colr` description where it has one, else the AV1 sequence header's -
/// libavif's order. An ICC-only `colr` says nothing about the matrix, so it falls through.
fn colour(parser: &AvifParser<'_>, planes: &av1::Planes) -> (u16, bool) {
    match parser.color_info() {
        Some(ColorInformation::Nclx {
            matrix_coefficients,
            full_range,
            ..
        }) => (*matrix_coefficients, *full_range),
        _ => (planes.matrix, planes.full_range),
    }
}

fn grid(parser: &AvifParser<'_>) -> ImageResult<RgbImage> {
    let config = parser
        .grid_config()
        .ok_or_else(|| failed("grid without a layout"))?;
    let columns = u32::from(config.columns);
    let count = parser.grid_tile_count();
    if count != usize::from(config.rows) * usize::from(config.columns) {
        return Err(failed("grid tile count does not match its layout"));
    }
    let mut canvas: Option<RgbImage> = None;
    let mut tile_size = (0, 0);
    for index in 0..count {
        let planes =
            av1::decode(&parser.tile_data(index).map_err(parse_failed)?).map_err(failed)?;
        let (matrix, full_range) = colour(parser, &planes);
        let tile = yuv::to_rgb(&planes, matrix, full_range);
        let canvas = match &mut canvas {
            Some(canvas) => canvas,
            None => {
                tile_size = tile.dimensions();
                let (w, h) = grid_size(config, tile_size);
                if w > columns * tile_size.0 || h > u32::from(config.rows) * tile_size.1 {
                    return Err(failed("grid larger than its tiles"));
                }
                canvas.insert(RgbImage::new(w, h))
            }
        };
        if tile.dimensions() != tile_size {
            return Err(failed("grid tiles differ in size"));
        }
        let (row, column) = (index as u32 / columns, index as u32 % columns);
        // `replace` clips at the canvas edge, which is the trim to the declared output size.
        imageops::replace(
            canvas,
            &tile,
            i64::from(column * tile_size.0),
            i64::from(row * tile_size.1),
        );
    }
    canvas.ok_or_else(|| failed("grid with no tiles"))
}

/// A grid's output size, or the tiles' extent when the file leaves it at zero.
fn grid_size(config: &GridConfig, (tile_w, tile_h): (u32, u32)) -> (u32, u32) {
    if config.output_width == 0 || config.output_height == 0 {
        (
            u32::from(config.columns) * tile_w,
            u32::from(config.rows) * tile_h,
        )
    } else {
        (config.output_width, config.output_height)
    }
}

/// `clap`, then `irot`, then `imir`: the order MIAF gives them.
fn transform(parser: &AvifParser<'_>, img: DynamicImage) -> DynamicImage {
    let img = match clean_aperture(parser, img.width(), img.height()) {
        Some((x, y, w, h)) => img.crop_imm(x, y, w, h),
        None => img,
    };
    // `irot` turns anticlockwise; `image` turns clockwise.
    let img = match parser.rotation().map(|r| r.angle) {
        Some(90) => img.rotate270(),
        Some(180) => img.rotate180(),
        Some(270) => img.rotate90(),
        _ => img,
    };
    // Axis 1 flips left-to-right and axis 0 top-to-bottom: libavif's reading, and so the
    // browsers'. zenavif-parse's doc comment on `ImageMirror` says the opposite; the
    // `imir.avif` fixture pins which one the webview will agree with.
    match parser.mirror().map(|m| m.axis) {
        Some(0) => img.flipv(),
        Some(1) => img.fliph(),
        _ => img,
    }
}

/// The `clap` crop as (x, y, width, height), or `None` when there is none, or when it is
/// not a whole-pixel rectangle inside the picture - libavif ignores such a crop too.
fn clean_aperture(parser: &AvifParser<'_>, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let c = parser.clean_aperture()?;
    let ratio = |n: f64, d: u32| (d != 0).then(|| n / f64::from(d));
    let crop_w = ratio(f64::from(c.width_n), c.width_d)?;
    let crop_h = ratio(f64::from(c.height_n), c.height_d)?;
    // The offsets are of the crop's centre from the picture's centre.
    let x = ratio(f64::from(c.horiz_off_n), c.horiz_off_d)? + (f64::from(w) - crop_w) / 2.0;
    let y = ratio(f64::from(c.vert_off_n), c.vert_off_d)? + (f64::from(h) - crop_h) / 2.0;
    let whole =
        |v: f64| (v >= 0.0 && v.fract() == 0.0 && v <= f64::from(u32::MAX)).then_some(v as u32);
    let (x, y, crop_w, crop_h) = (whole(x)?, whole(y)?, whole(crop_w)?, whole(crop_h)?);
    let fits = |start: u32, len: u32, total: u32| start.checked_add(len).is_some_and(|end| end <= total);
    let inside = crop_w > 0 && crop_h > 0 && fits(x, crop_w, w) && fits(y, crop_h, h);
    inside.then_some((x, y, crop_w, crop_h))
}

fn failed(message: impl Into<String>) -> ImageError {
    ImageError::Decoding(DecodingError::new(
        ImageFormatHint::Exact(ImageFormat::Avif),
        message.into(),
    ))
}

fn parse_failed(err: zenavif_parse::Error) -> ImageError {
    failed(err.to_string())
}
```

`as_chunks` needs Rust 1.88. It is what this toolchain's clippy asks for in place of
`chunks_exact(4)`, and the new minimum (1.93) covers it.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cargo test -p photon-core --lib avif`
Expected: every `avif::` test passes (Task 1's, Task 2's and these 14).

These expectations were checked on 2026-09-24 against a scratch build of exactly this code.
libavif's own decoder agrees on every point except flat blue under BT.709 limited range,
where libavif gives 243 and this code 255. The source was 255, and the ±12 tolerance covers
both.

- [ ] **Step 5: Revert probes**

Make each change, confirm the named test FAILS, then undo it exactly:
- In `is_avif`, drop `|| compatible.iter().any(is_avif_brand)` → `accepts_a_mif1_major_brand` fails.
- In `decode_avif`, replace `let bytes = with_avif_major(bytes);` with `let bytes = Cow::Borrowed(bytes);` →
  `accepts_a_mif1_major_brand` fails.
- In `colour`, replace the whole `match` with `(planes.matrix, planes.full_range)` → nothing
  should fail, because `avifenc` writes the same values into `colr` and the sequence header.
  This probe *passing* is the expected finding. Now replace the `_ =>` arm with
  `_ => (2, true)` → `falls_back_to_the_sequence_header_colour` fails.
- Swap `Some(90) => img.rotate270()` and `Some(270) => img.rotate90()` → `applies_irot` fails.
- Swap `Some(0) => img.flipv()` and `Some(1) => img.fliph()` → `applies_imir` fails.
- In `grid`, swap `row`/`column` in the `let (row, column) = …` line → `stitches_a_ten_bit_grid` fails.
- Make `clean_aperture` return `None` at its top → `applies_clap` fails.
- Change Task 2's `(y >> p.shift.1)` to `(y >> p.shift.0)` in `yuv.rs` →
  `subsamples_422_horizontally_only` fails. (That closes Task 2's open probe.)
- In `avif_dimensions`, drop the `Some(90 | 270) => (h, w)` arm → `applies_irot` fails on the
  `avif_dimensions` assertion.

- [ ] **Step 6: Rust gate, then commit**

```bash
git add crates/photon-core/src/avif
git commit -m "feat(avif): decode AVIF files - grids, alpha, clap/irot/imir, mif1 brands

<note the colour probe that passed and why, and the imir axis question>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Route every decode through one entry point, and index `.avif`

**Files:**
- Modify: `crates/photon-core/src/decode.rs`
- Modify: `crates/photon-core/src/edit.rs:238`
- Modify: `crates/photon-core/src/metadata.rs` (`read_header`, `read_image_meta_at`)
- Modify: `crates/photon-core/src/media.rs:18` and its extension test
- Modify: `crates/photon-core/src/scanner.rs` (test module)

**Interfaces:**
- Consumes: `avif::is_avif`, `avif::decode_avif`, `avif::avif_dimensions`.
- Produces: `pub fn decode::decode_image(path: &Path) -> Result<DynamicImage>`;
  `pub(crate) fn decode::dimensions<R: BufRead + Seek>(reader: &mut R) -> (Option<(u32, u32)>, bool)`,
  which returns the stored dimensions and whether the file is an AVIF.

- [ ] **Step 1: Write the failing tests**

In `crates/photon-core/src/decode.rs`'s test module, add `avif_fixture` and `jpeg_bytes` to the
`testutil` import, then:

```rust
    /// AVIF goes through photon's own decoder, whichever function the caller used.
    #[test]
    fn decodes_avif() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "a.avif", &avif_fixture("irot90.avif"));
        // Upright already: the container's rotation is part of the decode.
        let img = decode_oriented(&path, 1, 100).unwrap();
        assert_eq!((img.width(), img.height()), (32, 64));
        let (dims, avif) = dimensions(&mut BufReader::new(File::open(&path).unwrap()));
        assert_eq!((dims, avif), (Some((32, 64)), true));
    }

    /// Routing is by content, as for every other format: a JPEG named `.avif` is a JPEG,
    /// and an AVIF named `.jpg` is an AVIF.
    #[test]
    fn routes_by_content_not_extension() {
        let dir = tempfile::tempdir().unwrap();
        let jpeg = write_file(dir.path(), "really-a-jpeg.avif", &jpeg_bytes(40, 20));
        assert_eq!(decode_image(&jpeg).unwrap().width(), 40);
        let (dims, avif) = dimensions(&mut BufReader::new(File::open(&jpeg).unwrap()));
        assert_eq!((dims, avif), (Some((40, 20)), false));
        let avif = write_file(dir.path(), "really-an-avif.jpg", &avif_fixture("mono.avif"));
        assert_eq!(decode_image(&avif).unwrap().width(), 64);
    }

    /// A broken AVIF is a source defect (the failed-thumbnail placeholder), not an I/O error
    /// the thumbnail service would retry.
    #[test]
    fn decode_reports_a_corrupt_avif_as_an_image_error() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = avif_fixture("red_blue_709_limited.avif");
        let path = write_file(dir.path(), "cut.avif", &bytes[..bytes.len() / 2]);
        assert!(matches!(decode_oriented(&path, 1, 100), Err(Error::Image(_))));
    }
```

(Add `use std::fs::File; use std::io::BufReader;` to the test module if the parent module's
imports do not already bring them in.)

In `crates/photon-core/src/metadata.rs`'s test module, add `avif_fixture` to the `testutil`
import and:

```rust
    /// libavif writes a phone's EXIF orientation into `irot` and keeps the EXIF as it was, so
    /// this file says "rotate" twice. The decoder already applies `irot`; honouring the EXIF
    /// as well would turn the photo a second time. Dimensions are the displayed ones, and
    /// the date still comes from the EXIF.
    #[test]
    fn an_avif_is_oriented_by_its_container_not_its_exif() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_file(dir.path(), "phone.avif", &avif_fixture("exif_orientation6.avif"));
        let meta = read_image_meta(&path);
        assert_eq!((meta.width, meta.height, meta.orientation), (32, 64, 1));
        assert_eq!(meta.taken_at, Some(1_718_454_645));
    }
```

In `crates/photon-core/src/media.rs`'s `recognises_common_image_extensions_case_insensitively`,
add `"a.avif", "a.AVIF"` to the list.

In `crates/photon-core/src/scanner.rs`'s test module, add `avif_fixture` to the `testutil`
import and, after `indexes_tiff_and_bmp_files`:

```rust
    /// An AVIF is indexed beside a JPEG with its displayed size: the grid photo at its
    /// stitched size, the rotated one turned.
    #[test]
    fn indexes_avif_files() {
        let (dir, lib) = temp_library();
        let root = photos_root(&dir);
        let grid = write_file(&root, "grid.avif", &avif_fixture("grid_10bit.avif"));
        let turned = write_file(&root, "turned.avif", &avif_fixture("irot90.avif"));
        write_file(&root, "plain.jpg", &jpeg_bytes(8, 8));
        let watched = lib.add_watched_folder(&root, &[]).unwrap();

        let report = scan(&lib, &watched, 1);
        assert_eq!(report.added, 3);

        let known = lib.known_items(watched.id).unwrap();
        let shape = |path: &Path| {
            let item = lib.item(known[&key(path)].id).unwrap().unwrap();
            (item.width, item.height, item.orientation)
        };
        assert_eq!(shape(&grid), (128, 128, 1));
        assert_eq!(shape(&turned), (32, 64, 1));
    }
```

(`Item::orientation` is at `library/items.rs:24`.)

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p photon-core -- decodes_avif routes_by_content decode_reports_a_corrupt_avif an_avif_is_oriented recognises_common_image indexes_avif_files`
Expected: compile errors for `decode_image`/`dimensions` first. Add stubs so the suite
compiles: `decode_image` delegates to `ImageReader::open(path)?.with_guessed_format()?.decode()?`,
and `dimensions` returns `(None, false)`. Then every new test FAILS: `image` cannot decode
AVIF, the extension is not recognised, and the scan adds 1 file, not 3.

- [ ] **Step 3: Implement the routing in `decode.rs`**

Replace `decode.rs`'s `use` lines and add, above `decode_oriented`:

```rust
use crate::Result;
use crate::avif;
use image::{DynamicImage, ImageFormat, ImageReader, imageops::FilterType};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// How much of a file is read to tell an AVIF from anything else: enough for any `ftyp`
/// box a still-image writer produces, brands and all.
const SNIFF_BYTES: u64 = 256;

/// Whether the file under `reader` is an AVIF, leaving the reader where it was.
fn sniff_avif<R: Read + Seek>(reader: &mut R) -> std::io::Result<bool> {
    let start = reader.stream_position()?;
    let mut head = Vec::with_capacity(SNIFF_BYTES as usize);
    reader.by_ref().take(SNIFF_BYTES).read_to_end(&mut head)?;
    reader.seek(SeekFrom::Start(start))?;
    Ok(avif::is_avif(&head))
}

/// Decodes any photo photon indexes, recognising the format from its bytes. AVIF is photon's
/// own decoder (`avif`), since `image` reads it only through a C library; everything else is
/// `image`. Every full decode goes through here, so a new format is one branch in one place.
pub fn decode_image(path: &Path) -> Result<DynamicImage> {
    let mut reader = BufReader::new(File::open(path)?);
    if sniff_avif(&mut reader)? {
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

/// Stored dimensions from a reader at any position (it is rewound first), and whether the
/// file is an AVIF, whose orientation lives in its container rather than its EXIF. A header
/// read, not a decode, as `describe()` needs.
pub(crate) fn dimensions<R: BufRead + Seek>(reader: &mut R) -> (Option<(u32, u32)>, bool) {
    if reader.seek(SeekFrom::Start(0)).is_err() {
        return (None, false);
    }
    if sniff_avif(reader).unwrap_or(false) {
        let mut bytes = Vec::new();
        let dims = reader
            .read_to_end(&mut bytes)
            .ok()
            .and_then(|_| avif::avif_dimensions(&bytes));
        return (dims, true);
    }
    let dims = ImageReader::new(reader)
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok());
    (dims, false)
}
```

In `decode_oriented`, replace
`let img = ImageReader::open(path)?.with_guessed_format()?.decode()?;` with
`let img = decode_image(path)?;`.

`?` on `avif::decode_avif`'s `ImageError` converts through the existing
`Error::Image(#[from] image::ImageError)`. For an AVIF, `avif_dimensions` needs the whole
file (the parser reads to the end), unlike `image`'s header reads. It is still no decode, and
`describe()` runs it once per new or changed file.

- [ ] **Step 4: Use it in `edit::render_full`**

In `crates/photon-core/src/edit.rs:238`, replace
`let img = ImageReader::open(path)?.with_guessed_format()?.decode()?;` with
`let img = crate::decode::decode_image(path)?;`, and drop `ImageReader` from the
`use image::{DynamicImage, ImageReader};` line if nothing else in the file uses it.

- [ ] **Step 5: Use it in `metadata.rs`, and pin AVIF orientation**

In `read_header`, change the return type to `(Option<(u32, u32)>, Option<exif::Exif>, bool)`
and replace the `let dims = reader.seek(…)…;` block with:

```rust
    // `dimensions` rewinds first: the EXIF read consumed an unspecified amount, and a file
    // with no EXIF at all leaves the cursor wherever the attempt gave up.
    let (dims, avif) = crate::decode::dimensions(&mut reader);
    (dims, exif, avif)
```

and make its early return `(None, None, false)`. Update its doc comment's first line to
`/// Dimensions, EXIF and whether the file is an AVIF, from one open file.`

In `read_image_meta_at`, change `let (dims, exif) = read_header(path);` to
`let (dims, exif, avif) = read_header(path);`. In the orientation `if let`, add a guard and
a comment:

```rust
        // An AVIF is turned by its container's `irot`/`imir`, which the decoder applies and
        // `dims` already reflects. libavif keeps a phone's EXIF orientation beside the
        // `irot` it derives from it, so honouring both would turn the photo twice.
        if !avif
            && let Some(o) = exif
                .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .and_then(|f| f.value.get_uint(0))
            && (1..=8).contains(&o)
        {
            meta.orientation = o as u8;
        }
```

Remove the `Seek, SeekFrom` imports from `metadata.rs` if nothing else uses them (clippy will
say).

- [ ] **Step 6: Recognise the extension**

In `crates/photon-core/src/media.rs`, add `| "avif"` to the extension match:
`"jpg" | "jpeg" | "jpe" | "png" | "gif" | "webp" | "tif" | "tiff" | "bmp" | "avif" => {`.

- [ ] **Step 7: Run the tests to see them pass**

Run: `cargo test -p photon-core`
Expected: all pass, including every pre-existing decode, edit, metadata and scanner test.
Pay particular attention to `decode_reports_corrupt_and_missing_files` (a garbage `.jpg` must
still be `Error::Image`, a missing file still `Error::Io`) and the edit render tests.

- [ ] **Step 8: Revert probes**

Make each change, confirm the named test FAILS, then undo it exactly:
- In `decode_image`, delete the `if sniff_avif(…)? { … }` block → `decodes_avif` and
  `routes_by_content_not_extension` (the `.jpg` AVIF half) fail.
- In `metadata.rs`, delete `!avif &&` from the orientation guard →
  `an_avif_is_oriented_by_its_container_not_its_exif` fails with orientation 6.
- In `media.rs`, remove `| "avif"` → `indexes_avif_files` fails (1 added, not 3) and the
  extension test fails.
- In `edit::render_full`, put back `ImageReader::open(path)?.with_guessed_format()?.decode()?`
  → **no test fails yet.** Add this test to `edit.rs`'s test module, watch it fail with the
  old line and pass with the new:

```rust
    /// An edited AVIF renders through photon's own AVIF decoder: the viewer shows the
    /// render, not the file, whenever the photo is edited.
    #[test]
    fn renders_an_edited_avif() {
        let dir = tempfile::tempdir().unwrap();
        let path = crate::testutil::write_file(
            dir.path(),
            "a.avif",
            &crate::testutil::avif_fixture("red_blue_444_full.avif"),
        );
        let turn = Edit { turns: 1, crop: None };
        let (bytes, mime) = render_full(&path, 1, turn, FULL_QUALITY).unwrap();
        assert_eq!(mime, "image/jpeg");
        let out = image::load_from_memory(&bytes).unwrap();
        assert_eq!((out.width(), out.height()), (32, 64));
    }
```

(`Edit { turns, crop }` is the struct at `edit.rs:35`; `turns` is clockwise quarter turns.)

- [ ] **Step 9: Rust gate, then commit**

```bash
git add crates/photon-core/src
git commit -m "feat(avif): index .avif photos, one decode entry point for every caller

No EXIF_VERSION bump and no migration: describe() is unchanged for every existing format,
and a .avif was never indexed, so there is no stale row.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Serve `image/avif`, and document

**Files:**
- Modify: `crates/photon-app/src/protocol.rs:124-137` and its tests
- Modify: `README.md` (`### File formats`, `## Manual smoke checklist`)
- Modify: `CLAUDE.md` (Conventions, after "No native library dependencies")

- [ ] **Step 1: Write the failing test**

In `protocol.rs`'s test module, after `serves_originals_with_their_mime_type`:

```rust
    /// The unedited AVIF is served as itself: the webview decodes it natively where it can,
    /// and the viewer keeps the preview where it cannot.
    #[test]
    fn serves_avif_as_avif() {
        assert_eq!(mime_for(Path::new("a.avif")), "image/avif");
        assert_eq!(mime_for(Path::new("B.AVIF")), "image/avif");
    }
```

Run: `cargo test -p photon-app serves_avif_as_avif`
Expected: FAIL (`application/octet-stream`).

- [ ] **Step 2: Implement**

In `mime_for`, add `Some("avif") => "image/avif",` after the `webp` arm.

Run: `cargo test -p photon-app serves_avif_as_avif`
Expected: PASS. Revert probe: remove the arm and watch it fail, then put it back.

- [ ] **Step 3: README, file formats**

Replace the first paragraph of `### File formats` with:

```markdown
photon indexes JPEG, PNG, GIF, WebP, TIFF, BMP and AVIF. A TIFF holding several pages is shown
as its first page. AVIF is decoded by photon itself, in pure Rust, including the tiled and
10-bit photos phones write. An HDR AVIF is shown as standard range without tone mapping, so it
may look flat, and an animated one shows its still image. The viewer shows full-size AVIFs
where the system's web view can (Windows, macOS 13 and later, most Linux desktops) and the
1600-pixel preview elsewhere. Camera RAW files and HEIC are not read: every way of decoding
them means shipping a C library, and photon deliberately has no native dependencies.
```

and change the next paragraph's first line to `Adding TIFF, BMP and AVIF does not disturb a
library built by an earlier photon.`

- [ ] **Step 4: README, smoke checklist**

After the TIFF/BMP item in `## Manual smoke checklist`, add:

```markdown
- [ ] Drop AVIFs into a watched folder: one from a phone (tiled, rotated), one exported by an editor, one with transparency. All appear after the scan, upright, with thumbnails whose shape matches the tile. Open each in the viewer at 100% on Windows, macOS and Linux. The full-size picture must be framed the same as its thumbnail. On a system whose web view cannot show AVIF (macOS 12 or older), the viewer stays on the sharp preview with no error. Turn one and crop one: the edit shows in the grid and the viewer. A truncated `.avif` (cut a copy in half) shows the failed-thumbnail placeholder and does not stall its folder.
```

- [ ] **Step 5: CLAUDE.md**

After the "No native library dependencies" bullet in `## Conventions`, add:

```markdown
- **AVIF is decoded in `photon_core::avif`, not by `image`**, whose AVIF decoder is dav1d (C).
  `zenavif-parse` reads the container, `rav1d` (built without its assembly) decodes the AV1, and
  `avif/av1.rs` holds the only `unsafe` code in photon. Every full decode goes through
  `decode::decode_image`, which sniffs the `ftyp` box; calling `ImageReader` directly skips
  AVIF. The container's `irot`/`imir` are applied in the decoder, so an AVIF's stored
  orientation is always 1 and its EXIF orientation is ignored. `imir` axis 1 is left-to-right,
  as libavif reads it, whatever `zenavif-parse`'s doc comment says.
```

- [ ] **Step 6: Full gates**

Run the Rust gate (all five commands) and the UI gate (`npm run check`, `npm test`). The UI
does not change, but `npm run check` confirms nothing else did. Also run
`cargo run -p xtask -- metadata` and `cargo run -p xtask -- versions`, since the workspace
`Cargo.toml` changed.

Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add crates/photon-app/src/protocol.rs README.md CLAUDE.md
git commit -m "feat(avif): serve AVIF to the viewer as image/avif; document it

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 8: Whole-branch review**

Per CLAUDE.md, a branch this size gets an independent read before it merges. Point the
reviewer at:
- `av1.rs`'s `unsafe` blocks, especially the lifetimes of `Picture` and `seq_hdr`, and
  whether any path drops a picture it never got;
- the send/get loop's termination;
- anything this *arms* in old code: `decode_image` replacing `ImageReader::open` in three
  callers, which changes how a non-AVIF file is opened (one open, extension as fallback);
- the metadata change for non-AVIF files: `dimensions` rewinds and reads exactly as the old
  block did.

---

### Task 6: The crash-loop guard (added 2026-09-24, after the whole-branch review)

Approved in conversation on 2026-09-24. The spec's "A decoder panic aborts photon" limit left
one consequence open. A photo whose decode kills the process stays `Pending`, so the next
launch queues it again and dies again, with nothing naming the file. `catch_unwind` in
`thumbs/service.rs` cannot help with three kinds of death: a panic inside rav1d (it cannot
unwind out of rav1d's `extern "C"` entry points), an allocation failure (it aborts without
unwinding), and the OOM killer. A panic hook would see only the first. A marker file per
in-flight decode survives all three.

**Files:**
- Create: `crates/photon-core/src/thumbs/inflight.rs`
- Modify: `crates/photon-core/src/thumbs/mod.rs` (`mod inflight;`)
- Modify: `crates/photon-core/src/thumbs/cache.rs` (`pub(crate) fn root(&self) -> &Path`)
- Modify: `crates/photon-core/src/thumbs/service.rs` (`ThumbService`, `start_with`, `process`, the worker loop, `get_or_generate`, tests)
- Modify: `docs/superpowers/specs/2026-09-24-photon-avif-design.md` (the Limits bullet), `CLAUDE.md` (the AVIF paragraph's abort sentence)

**Interfaces:**
- Produces:
  - `pub(crate) struct InFlight` with `new(cache_root: &Path) -> Self`, `recover(&self)`, `deaths(&self, id: i64) -> u32`, `begin(&self, id: i64, deaths: u32) -> Marker` and `clear(&self, id: i64)`.
  - `pub(crate) struct Marker`, whose `Drop` removes the marker file.
  - `pub(crate) const DEATHS_TO_FAIL: u32 = 2` and `pub(crate) const CRASH_MESSAGE: &str = "photon closed unexpectedly while reading this photo"`.
- Consumes: `ThumbCache::root()` and `Library::set_thumb_state_if_unchanged`.

**Behaviour:**
- **The marker.** `in-flight/<item id>` under the thumbnail cache root holds a decimal count: the number of times photon has died while this photo was in flight.
- **At service start.** `start_with` calls `recover()` before spawning any worker. It adds one to the count in every marker a previous run left behind.
- **In `process`.** After the existing missing/`Failed` early return, `process` reads `deaths(id)`.
  - At `DEATHS_TO_FAIL` or more, it records `Failed` with `CRASH_MESSAGE` through `set_thumb_state_if_unchanged`, calls `clear(id)` and returns `Ok(())` without calling the render.
  - Otherwise it holds `begin(id, deaths)` across the existing `catch_unwind`, so the marker is removed whether the render succeeds, returns an error, or panics and is caught.
- **Failure to write or remove a marker.** It is logged with `tracing::warn!` and never fails the thumbnail. A cache that cannot be written loses the guard, not the photo.
- **Scope.** Every format. On-demand calls (`get_or_generate`) go through `process` too.

- [ ] **Step 1: Write the failing tests**

In `crates/photon-core/src/thumbs/inflight.rs`, next to the stubbed `InFlight`, add `#[cfg(test)] mod tests`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recover_counts_one_death_per_leftover_marker() {
        let dir = tempfile::tempdir().unwrap();
        let inflight = InFlight::new(dir.path());
        // A leaked guard stands in for an abort, which never runs `Drop`.
        std::mem::forget(inflight.begin(7, 0));
        std::mem::forget(inflight.begin(8, 1));
        inflight.recover();
        assert_eq!((inflight.deaths(7), inflight.deaths(8)), (1, 2));
        assert_eq!(inflight.deaths(9), 0, "no marker, no deaths");
    }

    /// A marker whose content is not a number (a torn write when the power went) still
    /// counts as a death rather than being ignored.
    #[test]
    fn an_unreadable_marker_counts_as_a_first_death() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("in-flight")).unwrap();
        std::fs::write(dir.path().join("in-flight/5"), "garbage").unwrap();
        let inflight = InFlight::new(dir.path());
        inflight.recover();
        assert_eq!(inflight.deaths(5), 1);
    }

    #[test]
    fn a_marker_disappears_when_its_guard_drops() {
        let dir = tempfile::tempdir().unwrap();
        let inflight = InFlight::new(dir.path());
        let marker = inflight.begin(3, 1);
        assert_eq!(inflight.deaths(3), 1);
        drop(marker);
        assert!(!dir.path().join("in-flight/3").exists());
    }
}
```

In `service.rs`'s test module, add:

```rust
    fn marker(dir: &TempDir, id: i64) -> std::path::PathBuf {
        dir.path().join("cache").join("in-flight").join(id.to_string())
    }

    /// Fails loudly if the service calls it: a photo marked failed by the guard must not be
    /// decoded again. A panic here is caught and recorded as "decoder panicked", which
    /// the tests below tell apart from the guard's own message.
    fn must_not_render(
        _: &ThumbCache,
        _: &Path,
        _: u8,
        _: Edit,
    ) -> Result<(DynamicImage, DynamicImage)> {
        panic!("the guard should have stopped this decode");
    }

    /// Renders only if a marker is on disk while it runs, which is the guard's whole point:
    /// the marker has to exist *during* the decode that might kill the process.
    fn render_requiring_a_marker(
        cache: &ThumbCache,
        source: &Path,
        orientation: u8,
        edit: Edit,
    ) -> Result<(DynamicImage, DynamicImage)> {
        let marked = std::fs::read_dir(cache.root().join("in-flight"))
            .is_ok_and(|mut entries| entries.next().is_some());
        assert!(marked, "no in-flight marker while decoding");
        default_render(cache, source, orientation, edit)
    }

    #[test]
    fn a_marker_is_on_disk_while_rendering_and_gone_after() {
        let (dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(40, 20))]);
        let service = ThumbService::start_with(lib.clone(), cache, 1, render_requiring_a_marker);
        service.get_or_generate(ids[0], ThumbSize::Grid).unwrap();
        assert_eq!(state(&lib, ids[0]), ThumbState::Ready);
        assert!(!marker(&dir, ids[0]).exists());
    }

    #[test]
    fn a_caught_panic_leaves_no_marker() {
        let (dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(40, 20))]);
        let service = ThumbService::start_with(lib.clone(), cache, 1, panicking_render);
        assert!(service.get_or_generate(ids[0], ThumbSize::Grid).is_err());
        assert!(!marker(&dir, ids[0]).exists());
    }

    /// Photon died once with this photo in flight (a marker at 0 left behind): that could be
    /// the user quitting, so the photo is decoded again, and succeeding clears the record.
    #[test]
    fn one_death_is_forgiven() {
        let (dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(40, 20))]);
        std::fs::create_dir_all(marker(&dir, ids[0]).parent().unwrap()).unwrap();
        std::fs::write(marker(&dir, ids[0]), "0").unwrap();
        let service = ThumbService::start_with(lib.clone(), cache, 1, default_render);
        service.get_or_generate(ids[0], ThumbSize::Grid).unwrap();
        assert_eq!(state(&lib, ids[0]), ThumbState::Ready);
        assert!(!marker(&dir, ids[0]).exists());
    }

    /// Photon died a second time with this photo in flight (its marker already recorded one
    /// death): the photo is failed with the guard's message, the decoder is not called, and
    /// the marker is cleared.
    #[test]
    fn two_deaths_fail_the_photo_without_decoding_it() {
        let (dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(40, 20))]);
        std::fs::create_dir_all(marker(&dir, ids[0]).parent().unwrap()).unwrap();
        std::fs::write(marker(&dir, ids[0]), "1").unwrap();
        let service = ThumbService::start_with(lib.clone(), cache, 1, must_not_render);
        assert!(service.get_or_generate(ids[0], ThumbSize::Grid).is_err());
        let item = lib.item(ids[0]).unwrap().unwrap();
        assert_eq!(item.thumb_state, ThumbState::Failed);
        assert_eq!(item.thumb_error.as_deref(), Some(CRASH_MESSAGE));
        assert!(!marker(&dir, ids[0]).exists());
    }
```

Adjust names to the test module's existing helpers (`setup`, `state`, `panicking_render`,
`default_render`, `TempDir`), which are all already there. `get_or_generate` on an item
already `Failed` returns `Err(ThumbFailed(thumb_error))`, which is what the last test's first
assertion relies on. Check that path in `get_or_generate` first.

Stub the new API (`InFlight` methods that do nothing, `deaths` returning 0, `ThumbCache::root`)
so the tests compile, then run them.

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p photon-core --lib thumbs`
Expected: the new tests FAIL on their assertions: no marker while rendering, a count that is
never recorded, and `two_deaths…` recording "decoder panicked" instead of `CRASH_MESSAGE`.

- [ ] **Step 3: Implement `inflight.rs`**

```rust
//! Remembers which photos a thumbnail worker was decoding when photon died, so a photo
//! that kills the process is not decoded again on every launch.
//!
//! `catch_unwind` in `service.rs` contains a decoder that panics, but three ways of dying
//! get past it:
//! - a panic inside rav1d, which cannot unwind out of its `extern "C"` entry points and
//!   aborts (see `avif/av1.rs`);
//! - an allocation failure, which aborts without unwinding;
//! - the OOM killer.
//!
//! Each leaves the photo `Pending`, so the next launch queues it again and dies again, with
//! nothing naming the file. A marker file per in-flight decode survives all three, where a
//! panic hook would see only the first.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// How many deaths with a photo in flight make it the suspect. One is not enough: quitting,
/// a power cut or an unrelated crash while a photo happens to be decoding would blame it.
pub(crate) const DEATHS_TO_FAIL: u32 = 2;

/// Shown where the photo's thumbnail would be, like any other decode failure.
pub(crate) const CRASH_MESSAGE: &str = "photon closed unexpectedly while reading this photo";

pub(crate) struct InFlight {
    dir: PathBuf,
}

impl InFlight {
    pub(crate) fn new(cache_root: &Path) -> Self {
        Self {
            dir: cache_root.join("in-flight"),
        }
    }

    /// Counts one death for every marker a previous run left behind. Called once, before any
    /// worker starts: a marker still present then was being decoded when the process ended.
    pub(crate) fn recover(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let deaths = read_count(&path).unwrap_or(0);
            if let Err(err) = fs::write(&path, (deaths + 1).to_string()) {
                tracing::warn!(%err, ?path, "could not record a death against an in-flight photo");
            }
        }
    }

    /// Deaths recorded against `id` so far; none if it has no marker.
    pub(crate) fn deaths(&self, id: i64) -> u32 {
        read_count(&self.path(id)).unwrap_or(0)
    }

    /// Marks `id` in flight until the returned guard drops. Best-effort: a cache that cannot
    /// be written loses the guard, not the thumbnail.
    pub(crate) fn begin(&self, id: i64, deaths: u32) -> Marker {
        let path = self.path(id);
        let written = fs::create_dir_all(&self.dir).and_then(|()| fs::write(&path, deaths.to_string()));
        if let Err(err) = written {
            tracing::warn!(%err, ?path, "could not mark a photo in flight");
        }
        Marker(path)
    }

    /// Forgets `id`'s deaths, once the photo has been failed for them.
    pub(crate) fn clear(&self, id: i64) {
        remove(&self.path(id));
    }

    fn path(&self, id: i64) -> PathBuf {
        self.dir.join(id.to_string())
    }
}

/// Removes its marker when dropped: on success, on an ordinary error, and on a panic that
/// `catch_unwind` caught, since unwinding runs `Drop`. An abort runs nothing, which is the
/// point: that marker stays behind for `recover` to count.
pub(crate) struct Marker(PathBuf);

impl Drop for Marker {
    fn drop(&mut self) {
        remove(&self.0);
    }
}

/// A marker that cannot be parsed (a torn write) is still a death: `Some(0)`, not `None`.
fn read_count(path: &Path) -> Option<u32> {
    let text = fs::read_to_string(path).ok()?;
    Some(text.trim().parse().unwrap_or(0))
}

fn remove(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => tracing::warn!(%err, ?path, "could not clear an in-flight marker"),
    }
}
```

Add `mod inflight;` to `thumbs/mod.rs`, and to `ThumbCache` add:

```rust
    /// The cache directory, for the in-flight markers kept beside the thumbnails.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
```

`collect_garbage` leaves the markers alone. It removes only `.webp` files and aged files whose
names start with `TEMP_PREFIX`, and a marker's name is a bare item id.

- [ ] **Step 4: Wire it into the service**

In `service.rs`:
- Add the field `inflight: Arc<InFlight>` to `ThumbService`.
- In `start_with`, before spawning workers:
  ```rust
          let inflight = Arc::new(InFlight::new(cache.root()));
          // Before any worker starts: a marker still here was in flight when a previous run died.
          inflight.recover();
  ```
  Clone it into each worker closure and pass `&inflight` to `process`. Store it in `Self`.
- In `get_or_generate`, pass `&self.inflight` to `process`.
- Change `process` to `fn process(lib: &Library, cache: &ThumbCache, inflight: &InFlight, id: i64, render: RenderFn) -> Result<()>`,
  and after the missing/`Failed` early return insert:
  ```rust
      let deaths = inflight.deaths(id);
      if deaths >= DEATHS_TO_FAIL {
          tracing::error!(id, path = %item.path, deaths, "photon died with this photo in flight; not decoding it again");
          lib.set_thumb_state_if_unchanged(&item, ThumbState::Failed, Some(CRASH_MESSAGE))?;
          inflight.clear(id);
          return Ok(());
      }
      // Held across the decode, so it is on disk if the decode takes the process down.
      let _marker = inflight.begin(id, deaths);
  ```
  followed by the existing `match catch_unwind(...)`.
- Extend `process`'s doc comment with one paragraph on the guard: which deaths it catches that
  `catch_unwind` does not, and the two-strike rule.

- [ ] **Step 5: Run the tests to see them pass**

Run: `cargo test -p photon-core --lib thumbs`
Expected: all pass, the existing thumbnail tests included.

- [ ] **Step 6: Revert probes**

Make each change, confirm the named test FAILS, then undo it exactly:
- Delete `inflight.recover();` → `two_deaths_fail_the_photo_without_decoding_it` fails with "decoder panicked".
- Change `DEATHS_TO_FAIL` to `1` → `one_death_is_forgiven` fails.
- Make `Marker::drop` empty → `a_marker_is_on_disk_while_rendering_and_gone_after` and `a_caught_panic_leaves_no_marker` fail.
- Make `begin` skip the write → `a_marker_is_on_disk_while_rendering_and_gone_after` fails ("no in-flight marker while decoding").
- Delete `inflight.clear(id);` → `two_deaths_fail_the_photo_without_decoding_it` fails on the marker assertion.
- In `read_count`, change `unwrap_or(0)` to `ok()?` style (unparseable → `None`) → `an_unreadable_marker_counts_as_a_first_death` fails.

- [ ] **Step 7: Docs**

- In the spec's "## Limits, stated rather than hidden", rewrite the "A decoder panic aborts photon" bullet. Keep its first part, then say the crash loop it would cause is guarded. Name the mechanism (a marker per in-flight thumbnail decode under the cache, a death counted per leftover marker at startup, the photo failed with `CRASH_MESSAGE` after two), and add what remains: the full-size render of an edited photo and export can still abort photon once, but only when the user asks, so they cannot loop.
- In `CLAUDE.md`'s AVIF bullet, change the abort sentence to point at the guard in one clause, e.g. "…aborts photon; `thumbs/inflight.rs` keeps that from repeating on every launch."
- In `README.md`'s smoke checklist, no item: an abort cannot be produced on demand.

- [ ] **Step 8: Rust gate, then commit**

Run the Rust gate, then:

```bash
git add crates/photon-core/src/thumbs docs/superpowers/specs/2026-09-24-photon-avif-design.md CLAUDE.md
git commit -m "feat(thumbs): stop a photo that kills photon from doing it on every launch

<the three deaths catch_unwind misses; the two-strike rule and why; the probe results>

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```
