# The native viewer, 4 of 8: colour

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Interfaces and
> tests are given, not every line: the plan is executed by its author, in one session.

**Goal:** a photo whose file says its colours are not sRGB - a Display P3 photo from a
phone, an Adobe RGB one from a camera - is shown in the native viewer in the colours it
was taken in, as a web view shows it, and not with its numbers read as sRGB.

**Architecture:** `photon_core::colour` (new) reads what a file says of its colours
(`embedded_profile`) and converts RGBA pixels to sRGB with moxcms (`to_srgb`).
`commands::viewer_picture` calls both for the picture it has rendered, before it scales
the fitted copy. Nothing else calls them: thumbnails and exports are as they were.

**Spec:** `docs/superpowers/specs/2026-10-12-photon-native-viewer-design.md` ("Colour";
"Decided while writing" 1; "Risks" 3; "Not decided here": an AVIF's `nclx`).

## Global constraints

- moxcms is pure Rust and in the build already, as `image`'s own dependency: photon-core
  names it, and `Cargo.lock` gains one line. No C, no system library.
- Nothing here writes anything. No photo file is opened for writing.
- Colours are converted in the viewer's full picture only. The preview, the grid's
  thumbnails, an export and the copy for the clipboard are not converted.
- The gate at every commit; every new test shown to fail with its rule broken.

## What was read first

- `image` 0.25.10: `ImageDecoder::icc_profile` answers for JPEG, PNG, TIFF and WebP;
  `JpegDecoder::new` reads the whole file. Its encoders for the four take a profile
  (`set_icc_profile`), which is how the tests make their files.
- moxcms 0.8.1: `ColorProfile::new_from_slice`, `create_transform_8bit` (`Layout::Rgba`:
  the alpha is passed through), `update_rgb_colorimetry_from_cicp`, and
  `new_display_p3` / `new_adobe_rgb` with `encode`, for the tests' profiles.
- `zenavif-parse` 0.6.2: `AvifParser::color_info` is the container's `colr` box, an ICC
  profile or the four numbers of an `nclx`.
- `photon_core::avif`: the matrix and the range are applied in the decoder (`yuv::to_rgb`);
  the primaries and the transfer are not looked at.
- `commands::viewer_picture`, `edit::render_picture`, `edit::fitted_copy`.

**Measured before the plan was written** (a scratch test, 24 million pixels of RGBA,
Display P3 to sRGB, one thread): 31 ms in a release build, 2.9 s unoptimised. In chunks,
each copied to a small buffer and converted back into its place, the same 30 ms - so the
conversion is made in place and no second picture of 96 MB is held. moxcms's answers for
six colours in each of the two profiles are, to the digit, those of the arithmetic done
by hand (primaries to XYZ, sRGB's own curve, Adobe's gamma of 563/256).

## Rulings made in the plan

- **A profile that leaves a spread of colours where they are is not applied.** 216
  colours (six steps of each channel) are converted first: if none moves by more than
  one step of 255, the picture is left alone. That is how "sRGB already" is told, with
  no list of names or of primaries - and it tells every profile that is sRGB in effect,
  however it is written.
- **RGB profiles only** (the spec). A CMYK or a grey profile is not applied.
- **An AVIF's `nclx` is converted where it is a standard-range colour space photon can
  name**: its primaries and its transfer curve, through moxcms's own table of them
  (Display P3 with sRGB's curve, which is what a phone's photo made into an AVIF says).
  **Not PQ and not HLG** (transfers 16 and 18): those are high dynamic range, which
  needs tone mapping to be shown on this surface at all; they are left as they are
  today. Nor "unspecified", which is taken for sRGB as a web view takes it.
- **The profile is read from the file by a read of its own**, after the decode, and not
  by threading it through `decode_image`: every full decode goes through that function,
  and its callers - thumbnails, hashes, exports - want no profile. For a JPEG `image`
  reads the whole file to find it, from the system's cache a moment after the decode
  did.
- **The fitted copy is scaled from the converted pixels**: converted and then scaled,
  not scaled and then converted, so that the two pictures of one photo are of the same
  colours to the digit.

## Review focus

1. A profile that is hostile or broken: truncated, huge, of another class (a device
   link, an abstract profile), with a LUT instead of a matrix. Nothing panics and the
   picture is shown, converted or as it is.
2. What is converted that should not be: a grey JPEG, a CMYK JPEG, a 16-bit PNG, a
   picture with alpha, an animated one.
3. The order of things in `viewer_picture`: converted before the fitted copy; the key;
   an edit; `uncropped`; the lock held across what.
4. Time and memory for a large photo; what a debug build pays.
5. Statements about colour that are not true.

## Tasks

### 1. `photon_core::colour`

`Profile` (a colour space that is not known to be sRGB), `embedded_profile(path) ->
Option<Profile>`, `to_srgb(rgba: &mut [u8], profile: &Profile) -> bool` (whether the
pixels were changed).

Tests: Display P3 and Adobe RGB colours against the values worked out by hand, alpha
kept; an sRGB profile changes nothing and says so; a JPEG, a PNG, a TIFF and a WebP made
with a profile give it back, and a file with none gives none; a broken profile, a CMYK
one and a grey one give none; an AVIF's ICC profile; an AVIF's `nclx` - Display P3,
BT.709, PQ, unspecified.

### 2. `commands::viewer_picture`

Tests: a Display P3 photo's full picture and its fitted copy are converted; turned, it
is converted too; a photo with no profile is as it was; the copy for the clipboard is
not converted. (The preview is not either; nothing holds it to that.)

### 3. The checklist, CLAUDE.md, the pull request

## Changed in the making

- **The profile is read through each format's own decoder**, made directly, and not
  through `ImageReader::into_decoder` as first written: through that, the test's TIFF
  gave no profile. (The review ran it to ground: the reader has the tiff crate refuse a
  tag with more values than the picture has bytes, which a 16-pixel fixture's profile
  has and a photo's has not. The decoders made directly stay, for the PNG's limit.)
- **`in_srgb(picture, path)`** is the one call the engine makes, added so that the
  engine names no type of `image`'s: `image` is an optional dependency there, for its
  tests' fixtures, and the first version built under test and would not have built
  without them.
- **An AVIF's numbers, in more detail than the ruling**: the curves of BT.709's family
  (1, 6, 14, 15) and "unspecified" are taken for sRGB's own, as a web view takes them;
  2.2, 2.8 and linear are their own; everything else - PQ and HLG among it - leaves the
  photo as it is. XYZ "primaries" are none.
- **moxcms 0.8.1's `update_rgb_colorimetry_from_cicp` answers false whatever it did**:
  the profile is made by `new_from_cicp` and asked whether it came out with primaries
  and curves.
- **A profile is applied only where it describes a device or a colour space** (its
  class), beside being RGB. The first tests of that made their profiles with moxcms's
  own writer, which for any class but a screen's or a camera's writes one that cannot
  be read back: such a profile was refused for being unreadable, the test passed before
  there was a rule, and proved nothing. They patch the header of a good profile now.
- **"sRGB already" is "within a step"**, with a test of its own. What the step is for
  is said in the code as far as it is known: a table of sRGB's curve, which is what the
  profile most files carry has, moves nothing at all under moxcms (tried at three
  sizes), so the step is for profiles that round sRGB's primaries their own way - and
  how common those are has not been measured.
- **Bytes that are no whole number of pixels are left alone**, said up front; a branch
  that put a refused chunk back was unreachable after that and went.
- **The extension is not asked** when the format is told: all four formats that carry
  a profile are known by their bytes.

**Probes**: thirty-one, each rule broken in turn with the test that was to fail. Two
passed the first time. One was "within a step", which no test could tell from "not at
all": it has its own test now. The other was no rule - the spread of colours tried at a
tolerance of sixty still tells Display P3 from sRGB, which says only that the spread is
generous.

**Not tested, and why**: a profile with a table for a whole colour space instead of
primaries (none can be made here without a real file); a real TIFF's or WebP's profile
written by another program (the fixtures are `image`'s own writing); the conversion's
effect on a screen (the window's surface, the checklist's business).

## What the review found

An independent read of the whole branch (a fresh reviewer, its own worktree and build),
against lcms2, libavif, ImageMagick and screenshots of headless Chromium, with a failing
test for each finding. The colour arithmetic agreed with lcms2 to within a step in every
space tried (Display P3, Adobe RGB, ProPhoto, BT.2020, a hand-built v4 profile and one
that is a table), and half a million mutated profiles made nothing panic or hang. Fixed
in the commit after the feature's unless said otherwise.

**To fix before merging**

- **A PNG's profile was inflated with no bound.** `PngDecoder::new` has no limits and a
  PNG's profile is deflated: a file of 4 MB took 1.3 GB to say it had no profile, and
  one of 40 MB would have taken the machine. The decode itself was bounded all along;
  this was the new read. It is read under 16 MB now.
- **A profile's `cicp` tag set its curve.** moxcms's default takes the curve from that
  tag: a profile with sRGB's curves and a tag saying PQ turned mid grey to near black -
  high dynamic range, in through a profile, against "PQ and HLG are left as they are" -
  and one saying BT.709 was read by BT.709's own formula, where the same words in an
  AVIF's box are taken for sRGB's. The curves are the curve tags' now. (Every profile
  the first tests used was written by moxcms, which puts that tag in: none of them took
  the road a phone's profile takes. The numbers were the same; the road was not.)
- **An AVIF with no `colr` box, whose AV1 stream names its colours, is not converted** -
  *left*. A browser reads the stream's own header; photon's decoder reads it for the
  matrix and the range, and this would mean reading the primaries and the curve out of
  the sequence header too, by hand or by a second decode. Writers put the box in
  (`avifenc` always does), so it is recorded and not built. If it is wrong, such a photo
  is shown as it was before this branch.

**Smaller, fixed**

- "Is it sRGB already" could be fooled: tried on six steps of each channel, a curve
  that is a table - sRGB's at those six, lifted between - passed with its greys
  seventeen steps out. Every step of grey and of each channel is tried now. (The cube
  of mixtures the first version also tried went: for a profile of primaries and curves
  the ramps decide, and nothing could tell the cube was there.)
- An AVIF that names no primaries and does name a curve was left as it was; each of the
  two has a default of its own, and it has BT.709's primaries now.
- Rules with no test: a number too large for the table (268 is 12 with a bit above),
  the two plain gammas and linear light, an AVIF under a generic major brand.
- Nothing held "nothing else is converted": the copy for the clipboard is asked now.

**Statements that were wrong, put right**: why a TIFF's profile was lost (above); what
moxcms's writer leaves out; "BT.709's own formula makes a photo darker" - lighter, 128
as 140; "where none of the spread moves, none of the picture's would"; that a grey
profile "no longer describes" its numbers - it does, and a browser applies it; "either
way" for the chunks' time (30 ms against 27).

**Found and left, for the user to weigh**

- A grey photo with a grey profile keeps its numbers (128 where a browser shows 146):
  the spec's "RGB profiles only".
- An AVIF's profile is passed over when a box of numbers stands before it: the parser
  hands over the first. libavif writes the profile first.
- A PNG that says its colours by `gAMA`, `cHRM` or `cICP` and carries no profile is
  taken for sRGB.
- A picture of sixteen bits is made eight before it is converted: a wide-gamut grey
  ramp lands on 234 of 256 levels.
- A real profile tints a few neutral greys by one step (12 of 256 under an Adobe RGB
  profile from another writer); moxcms's own tint none.
- **Outside this branch, and bearing on the decision it leaves open**: a re-encoded
  export of a Display P3 photo carries Display P3's numbers and no profile, so every
  other program shows it duller. That is how exports have always been.
