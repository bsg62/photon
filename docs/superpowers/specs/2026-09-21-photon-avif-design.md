# AVIF

2026-09-21

photon indexes JPEG, PNG, GIF, WebP, TIFF and BMP, and the README explains why RAW and HEIC
are absent: "every way of decoding them means shipping a C library, and photon deliberately
has no native dependencies." AVIF looked like the same answer. It is not, and this design
exists because the assumption was tested rather than repeated.

## The spike, and what it settled

Probed 2026-09-21 in a throwaway crate. Findings, all measured on a machine with **no nasm
installed**:

- **`image` 0.25's AVIF decode is out.** Its `avif-native` feature binds to libdav1d, a C
  library.
- **The viable stack is `gamut-avif` + `re_rav1d`.** `gamut-avif` (MIT OR Apache-2.0) walks
  the HEIF box structure, derives the primary item, handles alpha and the `irot`/`imir`/`clap`
  transformative properties, and converts to RGB; `re_rav1d` (BSD-2-Clause) is the Rust port
  of dav1d and does the AV1 decoding. They are joined by an `Av1StillDecoder` adapter of
  about fifty lines, which the spike wrote and ran. Both build with
  `--no-default-features` — no nasm, no `cc`, no C toolchain — in about four seconds.
- **It decodes correctly** at 0.3, 3, 12 and 24 megapixels. Dimensions exact; pixel error
  consistent with the encoder's own loss and flat across sizes, which is what a *correct*
  decoder looks like as opposed to one that is quietly mangling large images.
- **It costs about 6× a JPEG.** 12 MP ≈ 560 ms, 24 MP ≈ 1.1 s, single-threaded, against
  ~175 ms for photon's whole 24 MP JPEG thumbnail today.
- **`avif-rust` was rejected with evidence.** The tempting single-crate MIT option decodes
  640×480 and 3 MP and then **fails at 12 MP** with `tile_group byte alignment bit is not
  zero` — its AV1 decoder cannot handle the multi-tile bitstreams that camera-sized images
  produce. Anyone re-evaluating it should test at 12 MP before anything else.
- **`zenavif` and `heic` are AGPL-3.0-only.** photon is MIT.

Two things the spike did *not* settle and the implementation must: threaded decoding
(`set_n_threads(4)` failed against the spike's naive single `send_data`/`get_picture`
sequence — rav1d wants the proper pump loop), and 10-bit, below.

## Decisions taken before the design

- **10-bit AVIF is decoded**, via `gamut-avif`'s planar surface and our own conversion.
  `gamut-avif` refuses >8-bit RGBA presentation outright — *">8-bit RGBA presentation is not
  yet supported (use the planar surface)"* — so the alternative was a photo that indexes and
  then shows a broken tile. Every AVIF photon indexes is a photo it can display.

## Decoding

A new `photon-core/src/avif.rs`:

```rust
pub fn decode(bytes: &[u8]) -> Result<DynamicImage>
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)>
pub fn exif(bytes: &[u8]) -> Option<Vec<u8>>
```

`decode` parses with `AvifContainer`, and branches on the primary item's `av1C` bit depth:

- **8-bit** takes `decode_primary_rgba8`, which gives the container's own colour handling,
  alpha compositing and transformative properties for free.
- **>8-bit** takes `decode_item_planar` and converts in photon: full/limited range expansion,
  the coefficient matrix named by the file's `colr` box (BT.709 and BT.601 by their
  coefficients, BT.2020 by its), and a right-shift to 8 bits with rounding.

  Where the transfer characteristic is PQ or HLG — HDR — a plain shift produces a grey,
  flat picture, so those take a documented Reinhard-style roll-off to SDR. **This is the
  part of this design most likely to be subtly wrong**, because it is the part with no
  external oracle: a 10-bit SDR photo can be checked against a known encoding, an HDR one
  can only be checked against taste. The roll-off is therefore one small pure function with
  its own tests over synthetic values, kept apart from the YUV conversion, so a future
  correction changes one thing.

`decode_oriented` in `decode.rs` currently hands everything to `image`'s byte sniffing.
AVIF is not among `image`'s enabled features, so that sniff fails and would report a
corrupt file. It gains a branch **on the file's bytes, not its extension** — an ISO-BMFF
`ftyp` whose major or compatible brand is `avif`/`avis` — keeping the existing rule that
format comes from content. A `.jpg` that is really an AVIF therefore still opens, which is
the behaviour every other format already has here.

`decode_oriented`'s doc comment records that `image`'s 512 MiB per-decode allocation limit
is what bounds one decode, and that `MAX_WORKERS` is what bounds the sum. **Our AVIF path
is outside `image` and gets neither for free.** It refuses a file whose `av1C` dimensions
exceed the same effective bound before decoding anything, so the two paths agree — a header
claiming absurd dimensions is refused before allocation either way.

## EXIF

`read_header` reads EXIF with `exif::Reader::read_from_container`, which knows JPEG, TIFF,
PNG, WebP and HEIF containers. Whether it accepts this particular AVIF shape is not something
to assume: the implementation tries it first, and falls back to pulling the `Exif` item out
of the container with `avif::exif` and handing the payload to `exif::Reader::read_raw`.
Dimensions come from `avif::dimensions` rather than `image::ImageReader`, for the same reason
the decode branches.

Everything downstream is unchanged: camera, lens, `taken_at` and orientation arrive as
`ImageMeta` like any other format's.

**`EXIF_VERSION` is not bumped.** The backfill exists to re-read files whose stored metadata
predates the current reader; no AVIF has ever been indexed, so there is nothing to backfill.
They appear as each folder is walked again, exactly as TIFF and BMP did — the README already
has the paragraph, and it gains AVIF.

`MediaKind::from_path` gains `"avif"`. `avifs` (image sequences) is deliberately left out:
`gamut-avif` puts sequences permanently out of scope, and photon has no notion of one file
holding several photos anyway — the same reason a multi-page TIFF shows its first page.

## The cost, and where it lands

6× a JPEG is per file, once, on the existing thumbnail worker pool, so it does not block
anything a person is looking at. It does mean the first scan of an AVIF-heavy folder takes
visibly longer, and the README's File formats section says so rather than leaving it to be
discovered.

Decoding stays **single-threaded per file**. The pool is already the concurrency, and the
spike's threaded attempt failed against a naive pump; a correct multi-threaded rav1d driver
is work with no benefit here, since `MAX_WORKERS` decodes are already in flight.

The 1600 px preview and 256 px grid thumbnails are rendered from one decode, as now. Full-size
viewer renders go through `/image/<id>` and its `RENDERING` serialisation, unchanged — an
AVIF simply takes its second there, which is the price of the format.

## Fixtures

Two small `.avif` files checked in under `crates/photon-core/tests/fixtures/` — one 8-bit
4:2:0, one 10-bit — generated by us with `ravif`, provenance recorded beside them. They are
binary fixtures, which this project otherwise avoids, and the reason to accept them is that
the alternative is a `ravif`/`rav1e` dev-dependency pulled in to encode two files, which is a
large build cost on every test run for two constants. They are a few kilobytes: a thin strip
proves a resolution claim as well as a square does.

A third fixture is a **multi-tile** image, large enough to be tiled. That is the one that
caught `avif-rust`, and a test that only ever decodes a small AVIF would have shipped it.

## Tests

- `decodes_an_eight_bit_avif` — correct dimensions and a known pixel.
- `decodes_a_ten_bit_avif` — the planar path. Fails if the bit-depth branch is dropped, which
  would otherwise surface only as an error on a minority of real files.
- `decodes_a_multi_tile_avif` — the `avif-rust` failure, pinned.
- `sniffs_avif_by_bytes_not_extension` — an AVIF named `.jpg` decodes. Fails if the branch is
  keyed on the extension.
- `reads_exif_from_an_avif` — camera and `taken_at` come back. Fails if the item fallback is
  missing *and* `kamadak-exif` does not take the container directly, which is precisely the
  uncertainty this test exists to resolve rather than assume.
- `refuses_an_absurd_avif_header` — a doctored `av1C` is rejected before allocation.
- `ten_bit_sdr_matches_eight_bit` — the same picture encoded at both depths decodes to
  visually equal output (a mean absolute error bound). This is the test that discriminates a
  broken range or matrix, which is otherwise invisible as "the colours look a bit off".

## Repository chores

`THIRD-PARTY-NOTICES.md` gains `gamut-avif`, `gamut-core`, `re_rav1d` and their transitive
additions; `cargo run -p xtask -- metadata` runs in CI and fails otherwise. The AVIF entry
joins the README's File formats section along with the slower-first-scan note, and the
existing sentence about RAW and HEIC is corrected: the reason is a C library **for those
formats**, not a blanket impossibility — AVIF is the counter-example and the README should
not imply otherwise.

## Not in this design

AVIF **encoding** — photon writes new photo files only on export, and export copies bytes.
Image sequences (`avifs`). HEIC, which shares AVIF's container but needs an HEVC decoder, a
different problem with a worse licensing story. Any use of rav1d for video.
