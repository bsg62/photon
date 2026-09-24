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
| `progressive.avif` | 64×32 | red_blue, layered for progressive rendering; must decode at full size, not the low-resolution base layer | `--progressive` |
| `grid_padded.avif` | 129×129 | 2×2 grid of 65×65 tiles (130×130 physical canvas) trimmed to a 129×129 output that is not a multiple of the tile size - the normal phone-camera shape; four quadrants: red, lime / blue, white | `magick` a 129×129 four-quadrant PNG (`-fill "#RRGGBB"`, not a named colour: ImageMagick's `green` is X11 `(0,128,0)`, not `(0,255,0)`), then `avifenc -s 8 -q 90 -y 444 --grid 2x2 src.png grid_padded.avif` from the single source image, letting libavif split and pad it. `-y 420` was tried first (matching the other grid fixture) but this libavif (1.4.2) refuses an odd overall width/height under 4:2:0 subsampling ("shall be even ... MIAF 7.3.11.4.2"), so this fixture is 4:4:4 instead - orthogonal to what it proves (the ImageGrid payload's declared, non-tile-multiple output size). |

The full script is in `docs/superpowers/plans/2026-09-24-photon-avif.md`, Task 1.
