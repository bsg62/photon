//! What a photo's file says of its colours, and the conversion of its pixels to sRGB.
//!
//! A decoder hands over numbers. What colours they are is said beside them: by an ICC
//! profile in a JPEG, a PNG, a TIFF or a WebP, and in an AVIF by a profile or by four
//! numbers of its own (`nclx`). A photo from a phone is Display P3 and one from a camera
//! often Adobe RGB, and their numbers read as sRGB are duller colours than were taken.
//! A web view converts them; the native viewer, which draws the pixels itself, asks here.
//!
//! For the viewer's full picture alone (`commands::viewer_picture`). Thumbnails, exports
//! and the copy for the clipboard are not converted: that is a decision about every
//! photo photon has ever made a picture of, and it has not been taken.

use crate::avif;
use image::codecs::{jpeg::JpegDecoder, png::PngDecoder, tiff::TiffDecoder, webp::WebPDecoder};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use moxcms::{
    CicpColorPrimaries, CicpProfile, ColorProfile, DataColorSpace, Layout, MatrixCoefficients,
    ProfileClass, TransferCharacteristics, TransformOptions,
};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

/// The colour space a file says its pixels are in: RGB, and not known to be sRGB until
/// its colours are converted (`to_srgb` says whether it changed anything).
pub struct Profile(ColorProfile);

impl Profile {
    /// An ICC profile, where it is one moxcms can read, its colours are RGB, and it
    /// describes the colours of a device or of a colour space: a screen, a camera or a
    /// scanner, a printer. Not a link between two devices, an abstract profile or a
    /// list of named colours, which say nothing of what a picture's numbers are.
    fn from_icc(icc: &[u8]) -> Option<Self> {
        let profile = ColorProfile::new_from_slice(icc).ok()?;
        let describes = matches!(
            profile.profile_class,
            ProfileClass::DisplayDevice
                | ProfileClass::InputDevice
                | ProfileClass::OutputDevice
                | ProfileClass::ColorSpace
        );
        (describes && profile.color_space == DataColorSpace::Rgb).then_some(Self(profile))
    }

    /// The colour space an AVIF names by number (ITU-T H.273's): its primaries and its
    /// transfer curve, where both are ones of standard range that moxcms has in its
    /// table. The matrix and the range are the decoder's business, and done.
    ///
    /// Not PQ and not HLG: high dynamic range is not shown on this surface by a
    /// conversion of numbers, and is left as it is. Primaries that are not said are
    /// BT.709's, which are sRGB's. And the curves of BT.709's family (1, 6, 14, 15),
    /// like no curve said at all (2), are taken for sRGB's own, as a web view takes
    /// them (Chromium does, for BT.709, SMPTE 170M and both of BT.2020's): by its own
    /// formula a photo that says "BT.709" comes out lighter than every browser shows
    /// it, 128 as 140. Beside those, the two plain gammas (2.2 and 2.8) and a linear
    /// one.
    fn from_cicp(primaries: u16, transfer: u16) -> Option<Self> {
        let color_primaries = match CicpColorPrimaries::try_from(u8::try_from(primaries).ok()?) {
            // XYZ is no colour space a photo is in.
            Ok(CicpColorPrimaries::Xyz) | Err(_) => return None,
            // Not said: BT.709's, which are sRGB's. Each of the two has a default of its
            // own, so a curve that is said still counts - linear light with no
            // primaries named is linear BT.709, and a web view shows it so.
            Ok(CicpColorPrimaries::Unspecified) => CicpColorPrimaries::Bt709,
            Ok(primaries) => primaries,
        };
        let transfer_characteristics = match transfer {
            1 | 2 | 6 | 13 | 14 | 15 => TransferCharacteristics::Srgb,
            4 => TransferCharacteristics::Bt470M,
            5 => TransferCharacteristics::Bt470Bg,
            8 => TransferCharacteristics::Linear,
            // PQ, HLG, and curves nobody takes photos in.
            _ => return None,
        };
        let profile = ColorProfile::new_from_cicp(CicpProfile {
            color_primaries,
            transfer_characteristics,
            matrix_coefficients: MatrixCoefficients::Identity,
            full_range: true,
        });
        // (Not for primaries its table does not hold: "unspecified" and "reserved"
        // leave a profile with none. `update_rgb_colorimetry_from_cicp` would say so
        // itself, and in moxcms 0.8.1 answers false whatever it did.)
        profile.is_matrix_shaper().then_some(Self(profile))
    }
}

/// How many pixels are converted at a time: each chunk is copied aside and converted
/// back into its place, so that no second picture is held - 96 MB for a camera's photo.
/// At this size the copy stays in the processor's cache, and the whole is nearly as
/// fast as one pass over two pictures (measured, 24 million pixels: 30 ms against 27).
const CHUNK_PIXELS: usize = 65_536;

/// The most memory a PNG's decoder may take to find the file's profile. No profile is
/// near it - a printer's, with its tables, is a few megabytes, and moxcms reads none of
/// ten or more.
const PROFILE_LIMIT: u64 = 16 * 1024 * 1024;

/// What `path`'s file says of its colours, where it says something photon applies: an
/// RGB profile, or an AVIF's standard-range colour space. None for a file that says
/// nothing - it is taken for sRGB, as a web view takes it - and for one that cannot be
/// read, whose profile is broken, or whose profile is of another kind of colour. A CMYK
/// one describes numbers the decoder has already made RGB of by a rule of its own, and
/// no longer describes them. A grey one still does - the decoder writes each number
/// three times and changes none - and a web view applies it: photon does not, which is
/// the spec's "RGB profiles only" and costs a grey photo with a profile of its own the
/// tones it was given (mid grey 128 is 146 under a common one).
///
/// A read of its own, after the decode and not part of it: every full decode goes
/// through `decode::decode_image`, and its other callers want no profile.
pub fn embedded_profile(path: &Path) -> Option<Profile> {
    match said(path)? {
        avif::Colours::Icc(icc) => Profile::from_icc(&icc),
        avif::Colours::Named {
            primaries,
            transfer,
        } => Profile::from_cicp(primaries, transfer),
    }
}

/// What `path`'s file carries about its colours, as it stands in the file: a profile's
/// bytes, or an AVIF's numbers.
fn said(path: &Path) -> Option<avif::Colours> {
    let mut reader = BufReader::new(File::open(path).ok()?);
    // By the file's bytes, as `decode_image` tells an AVIF from the rest.
    if avif::is_avif(reader.fill_buf().ok()?) {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).ok()?;
        return avif::colour_information(&bytes);
    }
    // (By its bytes alone: a file whose bytes do not say what it is is none of the four
    // formats that carry a profile.)
    let image = ImageReader::new(reader).with_guessed_format().ok()?;
    let format = image.format()?;
    let reader = image.into_inner();
    // Each format's own decoder, made directly, for the four that carry a profile: a
    // PNG's wants a limit of its own (below). (`ImageReader::into_decoder` would save
    // the list, and loses the profile of a *small* TIFF: it has the tiff crate take no
    // tag with more values than the picture has bytes, which a 16-pixel picture's
    // profile has and a photo's has not. This module's fixtures are that small.)
    let icc = match format {
        ImageFormat::Jpeg => (JpegDecoder::new(reader).ok()?.icc_profile()).ok()??,
        // A PNG's profile is deflated, and inflated in memory with whatever limit the
        // decoder is given: `PngDecoder::new` gives it none, and a file of four
        // megabytes then took 1.3 GB to say it had no profile.
        ImageFormat::Png => {
            let mut limits = image::Limits::default();
            limits.max_alloc = Some(PROFILE_LIMIT);
            let mut decoder = PngDecoder::with_limits(reader, limits).ok()?;
            decoder.icc_profile().ok()??
        }
        ImageFormat::Tiff => (TiffDecoder::new(reader).ok()?.icc_profile()).ok()??,
        ImageFormat::WebP => (WebPDecoder::new(reader).ok()?.icc_profile()).ok()??,
        _ => return None,
    };
    Some(avif::Colours::Icc(icc))
}

/// Converts straight RGBA pixels from `profile`'s colours to sRGB, in place, the alpha
/// left as it is. Answers whether anything was changed: a profile that is sRGB in
/// effect - one under which a spread of colours comes out where it went in, to within a
/// step - is not applied, and one moxcms will not make a conversion from is not either.
pub fn to_srgb(rgba: &mut [u8], profile: &Profile) -> bool {
    let srgb = ColorProfile::new_srgb();
    // The curves are the ones the profile carries (or `from_cicp` made), and not the one
    // a `cicp` tag in the profile names, which is moxcms's default: by that, a profile
    // with sRGB's curves and a tag saying PQ turned mid grey to near black - high
    // dynamic range, come in through a profile - and one saying BT.709 was read by
    // BT.709's own formula, where the same words in an AVIF's box are taken for sRGB's.
    let options = TransformOptions {
        allow_use_cicp_transfer: false,
        ..TransformOptions::default()
    };
    let Ok(conversion) =
        (profile.0).create_transform_8bit(Layout::Rgba, &srgb, Layout::Rgba, options)
    else {
        return false;
    };
    // A spread of colours first, and where none of them moves the picture is left as it
    // is. This is how a profile that is sRGB is told - by what it does, whatever it is
    // called and however it is written - and it is nearly every profile there is.
    let spread = spread();
    let mut moved = vec![0; spread.len()];
    if conversion.transform(&spread, &mut moved).is_err() {
        return false;
    }
    let same = unmoved(&spread, &moved);
    // (Nor bytes that are no whole number of pixels: no picture, and the one thing the
    // conversion below refuses.)
    if same || rgba.is_empty() || !rgba.len().is_multiple_of(4) {
        return false;
    }
    let mut aside = vec![0; CHUNK_PIXELS * 4];
    for chunk in rgba.chunks_mut(CHUNK_PIXELS * 4) {
        let aside = &mut aside[..chunk.len()];
        aside.copy_from_slice(chunk);
        let _ = conversion.transform(aside, chunk);
    }
    true
}

/// The colours a profile is tried on, as RGBA: every step of grey and of each channel
/// alone. For a profile of primaries and curves those decide it - a curve that is
/// sRGB's at all 256 steps is sRGB's, and primaries that leave the three channels where
/// they are leave every mixture of them. (Six steps of each were tried at first, and a
/// curve that was a table - sRGB's at those six, lifted in the shadows between - was
/// taken for sRGB with its greys seventeen steps out.) A profile that is a table of the
/// whole space could in principle keep all of these and move a mixture; none that does
/// is known, and one would be shown as its numbers are.
fn spread() -> Vec<u8> {
    (0..=255u8)
        .flat_map(|v| [[v, v, v], [v, 0, 0], [0, v, 0], [0, 0, v]])
        .flat_map(|[r, g, b]| [r, g, b, 255])
        .collect()
}

/// Whether a conversion left colours where they were: none of their numbers moved by
/// more than a step. A step, and not none, so that a profile which says sRGB in numbers
/// of its own rounding is still told for sRGB: one step of 255 is nothing anyone sees,
/// and not worth a conversion of every photo that carries such a profile. (How often
/// that is has not been measured. A profile that says sRGB's curve as a table, as the
/// one most files carry does, moves nothing at all under moxcms: tried with tables of
/// 256, 1,024 and 4,096 entries.)
fn unmoved(was: &[u8], is: &[u8]) -> bool {
    (was.iter().zip(is)).all(|(was, is)| was.abs_diff(*is) <= 1)
}

/// `picture`, decoded from `path`'s file, as straight RGBA in sRGB: converted where the
/// file says its colours are another space's, and as its numbers are where it says
/// nothing photon applies.
pub fn in_srgb(picture: DynamicImage, path: &Path) -> DynamicImage {
    let mut rgba = picture.into_rgba8();
    if let Some(profile) = embedded_profile(path) {
        to_srgb(&mut rgba, &profile);
    }
    DynamicImage::ImageRgba8(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::avif_fixture;
    use image::{ExtendedColorType, ImageEncoder};

    fn display_p3() -> Vec<u8> {
        ColorProfile::new_display_p3().encode().unwrap()
    }

    fn adobe_rgb() -> Vec<u8> {
        ColorProfile::new_adobe_rgb().encode().unwrap()
    }

    fn of(icc: &[u8]) -> Profile {
        Profile::from_icc(icc).expect("a profile photon applies")
    }

    /// Six colours with an alpha each, and what they are in sRGB when they are read as
    /// Display P3 and as Adobe RGB (1998). Worked out by hand, not by moxcms: each
    /// channel through its space's curve (sRGB's own for Display P3, a gamma of 563/256
    /// for Adobe), the three through the space's primaries to XYZ and through sRGB's
    /// back, then sRGB's curve, cut off at the ends where sRGB has no such colour.
    const COLOURS: [[u8; 4]; 6] = [
        [200, 100, 50, 255],
        [255, 0, 0, 128],
        [10, 200, 30, 0],
        [128, 128, 128, 255],
        [40, 90, 160, 7],
        [230, 220, 60, 255],
    ];
    const AS_DISPLAY_P3: [[u8; 3]; 6] = [
        [215, 93, 31],
        [255, 0, 0],
        [0, 204, 0],
        [128, 128, 128],
        [10, 91, 165],
        [232, 220, 0],
    ];
    const AS_ADOBE_RGB: [[u8; 3]; 6] = [
        [227, 100, 42],
        [255, 0, 0],
        [0, 201, 0],
        [129, 129, 129],
        [0, 90, 164],
        [234, 221, 29],
    ];

    fn converted(profile: &Profile) -> (bool, Vec<[u8; 4]>) {
        let mut pixels = COLOURS.concat();
        let changed = to_srgb(&mut pixels, profile);
        (changed, pixels.as_chunks::<4>().0.to_vec())
    }

    fn with_alpha(colours: &[[u8; 3]; 6]) -> Vec<[u8; 4]> {
        (colours.iter().zip(COLOURS))
            .map(|(colour, was)| [colour[0], colour[1], colour[2], was[3]])
            .collect()
    }

    #[test]
    fn display_p3_and_adobe_rgb_come_out_as_the_srgb_worked_out_by_hand() {
        assert_eq!(
            converted(&of(&display_p3())),
            (true, with_alpha(&AS_DISPLAY_P3))
        );
        assert_eq!(
            converted(&of(&adobe_rgb())),
            (true, with_alpha(&AS_ADOBE_RGB))
        );
    }

    // More pixels than are converted at a time, and not a whole number of chunks: every
    // one is converted, once, and the picture is as long as it was.
    #[test]
    fn a_picture_longer_than_a_chunk_is_converted_to_its_last_pixel() {
        let count = 2 * CHUNK_PIXELS + 3;
        let mut pixels = [200u8, 100, 50, 255].repeat(count);
        assert!(to_srgb(&mut pixels, &of(&display_p3())));
        assert_eq!(pixels.len(), count * 4);
        assert!(
            (pixels.as_chunks::<4>().0.iter()).all(|p| *p == [215, 93, 31, 255]),
            "every pixel"
        );
        // And nothing of a picture with no pixels, or of bytes that are no whole number
        // of pixels, which are left as they are.
        assert!(!to_srgb(&mut [], &of(&display_p3())));
        let mut odd = [200u8, 100, 50, 255, 200, 100];
        assert!(!to_srgb(&mut odd, &of(&display_p3())));
        assert_eq!(odd, [200, 100, 50, 255, 200, 100]);
    }

    #[test]
    fn a_profile_that_is_srgb_changes_nothing_and_says_so() {
        let srgb = ColorProfile::new_srgb().encode().unwrap();
        let (changed, pixels) = converted(&of(&srgb));
        assert!(!changed);
        assert_eq!(pixels, COLOURS);
    }

    #[test]
    fn colours_are_where_they_were_when_none_moved_by_more_than_a_step() {
        assert!(unmoved(&[10, 200, 30, 255], &[10, 200, 30, 255]));
        assert!(unmoved(&[10, 200, 30, 255], &[11, 199, 30, 255]));
        assert!(!unmoved(&[10, 200, 30, 255], &[12, 200, 30, 255]));
        assert!(!unmoved(&[10, 200, 30, 255], &[10, 200, 28, 255]));
    }

    /// A picture of `COLOURS`' first colour, written to `path` in `format` with `icc`.
    fn write(path: &Path, format: ImageFormat, icc: Option<&[u8]>) {
        let pixels = [200u8, 100, 50].repeat(16 * 16);
        let mut bytes = Vec::new();
        macro_rules! encode {
            ($encoder:expr) => {{
                let mut encoder = $encoder;
                if let Some(icc) = icc {
                    encoder.set_icc_profile(icc.to_vec()).unwrap();
                }
                (encoder.write_image(&pixels, 16, 16, ExtendedColorType::Rgb8)).unwrap();
            }};
        }
        match format {
            ImageFormat::Jpeg => encode!(image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut bytes, 95
            )),
            ImageFormat::Png => encode!(image::codecs::png::PngEncoder::new(&mut bytes)),
            ImageFormat::Tiff => encode!(image::codecs::tiff::TiffEncoder::new(
                std::io::Cursor::new(&mut bytes)
            )),
            ImageFormat::WebP => {
                encode!(image::codecs::webp::WebPEncoder::new_lossless(&mut bytes))
            }
            other => panic!("no such fixture: {other:?}"),
        }
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn a_files_profile_is_read_from_a_jpeg_a_png_a_tiff_and_a_webp() {
        let dir = tempfile::tempdir().unwrap();
        for (name, format) in [
            ("a.jpg", ImageFormat::Jpeg),
            ("a.png", ImageFormat::Png),
            ("a.tiff", ImageFormat::Tiff),
            ("a.webp", ImageFormat::WebP),
        ] {
            let path = dir.path().join(name);
            write(&path, format, Some(&display_p3()));
            let profile = embedded_profile(&path).unwrap_or_else(|| panic!("{name}: none"));
            assert_eq!(
                converted(&profile),
                (true, with_alpha(&AS_DISPLAY_P3)),
                "{name}"
            );
            // By what the file is, not by what it is called.
            let renamed = dir.path().join(format!("{name}.dat"));
            std::fs::rename(&path, &renamed).unwrap();
            assert!(embedded_profile(&renamed).is_some(), "{name}, renamed");
        }
    }

    #[test]
    fn a_file_that_says_nothing_of_its_colours_has_no_profile() {
        let dir = tempfile::tempdir().unwrap();
        for (name, format) in [("a.jpg", ImageFormat::Jpeg), ("a.png", ImageFormat::Png)] {
            let path = dir.path().join(name);
            write(&path, format, None);
            assert!(embedded_profile(&path).is_none(), "{name}");
        }
        // Nor one that is not there, or is no picture.
        assert!(embedded_profile(&dir.path().join("gone.jpg")).is_none());
        let text = dir.path().join("notes.png");
        std::fs::write(&text, b"not a picture").unwrap();
        assert!(embedded_profile(&text).is_none());
    }

    // A profile that is broken, cut short, or of another kind of colour than RGB is not
    // applied: the picture is shown as its numbers are.
    #[test]
    fn a_broken_profile_a_cmyk_one_and_a_grey_one_are_not_applied() {
        let dir = tempfile::tempdir().unwrap();
        // (A profile says what kind of colour it is of in four letters of its header,
        // and what kind of profile it is in four more.)
        let of_space = |space: &[u8; 4]| {
            let mut icc = display_p3();
            icc[16..20].copy_from_slice(space);
            icc
        };
        // Nor one that is no description of a device's or a space's colours at all: a
        // link between two devices, an effect to lay over a picture, a list of named
        // inks. (A profile says which it is in four letters of its header.)
        let of_class = |class: &[u8; 4]| {
            let mut icc = display_p3();
            icc[12..16].copy_from_slice(class);
            icc
        };
        let whole = display_p3();
        for (what, icc) in [
            ("nothing but noughts", vec![0u8; 300]),
            ("cut short", whole[..whole.len() / 2].to_vec()),
            ("a few bytes", vec![1, 2, 3]),
            ("CMYK", of_space(b"CMYK")),
            ("grey", of_space(b"GRAY")),
            ("Lab", of_space(b"Lab ")),
            ("a device link", of_class(b"link")),
            ("an abstract profile", of_class(b"abst")),
            ("named colours", of_class(b"nmcl")),
        ] {
            let path = dir.path().join("a.png");
            write(&path, ImageFormat::Png, Some(&icc));
            assert!(embedded_profile(&path).is_none(), "{what}");
            assert!(Profile::from_icc(&icc).is_none(), "{what}");
        }
        // (The fixture's own way is sound: whole, RGB and of a device or a space, it is
        // applied - a screen's, a camera's or a scanner's, a printer's, a colour
        // space's own.)
        for class in [b"mntr", b"scnr", b"prtr", b"spac"] {
            let path = dir.path().join("a.png");
            write(&path, ImageFormat::Png, Some(&of_class(class)));
            let class = String::from_utf8_lossy(class);
            assert!(embedded_profile(&path).is_some(), "{class}");
        }
    }

    /// An AVIF whose `colr` box says `primaries` and `transfer` (ITU-T H.273's numbers).
    fn avif_saying(dir: &Path, primaries: u16, transfer: u16) -> std::path::PathBuf {
        let mut bytes = avif_fixture("red_blue_709_limited.avif");
        let at = (bytes.windows(8))
            .position(|window| window == b"colrnclx")
            .expect("the fixture's nclx");
        bytes[at + 8..at + 10].copy_from_slice(&primaries.to_be_bytes());
        bytes[at + 10..at + 12].copy_from_slice(&transfer.to_be_bytes());
        let path = dir.join(format!("{primaries}-{transfer}.avif"));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    // An AVIF that says its colours by numbers: Display P3's primaries with sRGB's curve
    // is what a phone's photo says, and is converted as the profile of that name is.
    #[test]
    fn an_avif_that_names_a_standard_range_colour_space_is_converted() {
        let dir = tempfile::tempdir().unwrap();
        let p3 = embedded_profile(&avif_saying(dir.path(), 12, 13)).expect("Display P3");
        assert_eq!(converted(&p3), (true, with_alpha(&AS_DISPLAY_P3)));
        // The curves of BT.709's family, and none said, are taken for sRGB's, as a web
        // view takes them: read by their own formula, every photo that says "BT.709"
        // would come out darker than a browser shows it.
        for transfer in [1, 2, 6, 14, 15] {
            let p3 = embedded_profile(&avif_saying(dir.path(), 12, transfer));
            let p3 = p3.unwrap_or_else(|| panic!("transfer {transfer}"));
            assert_eq!(
                converted(&p3),
                (true, with_alpha(&AS_DISPLAY_P3)),
                "{transfer}"
            );
        }
        // BT.709's own primaries are sRGB's: nothing to convert.
        for transfer in [1, 13] {
            let same = embedded_profile(&avif_saying(dir.path(), 1, transfer)).expect("BT.709");
            assert_eq!(converted(&same), (false, COLOURS.to_vec()), "{transfer}");
        }
        // BT.2020's are not, and move a colour further than Display P3's do.
        let wide = embedded_profile(&avif_saying(dir.path(), 9, 13)).expect("BT.2020");
        let (changed, pixels) = converted(&wide);
        assert!(changed);
        assert!(pixels[0][0] > AS_DISPLAY_P3[0][0], "{:?}", pixels[0]);
    }

    // High dynamic range - PQ and HLG - is not a colour space this surface can show by
    // a conversion of numbers: it is left as it is. So is an AVIF whose primaries or
    // whose curve are ones nobody knows.
    #[test]
    fn an_avif_of_high_dynamic_range_or_of_primaries_nobody_knows_is_left_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        // (One that names no primaries has BT.709's, which with sRGB's curve or none
        // named is sRGB: a profile, and nothing to convert.)
        for transfer in [13, 2] {
            let unnamed = embedded_profile(&avif_saying(dir.path(), 2, transfer));
            let unnamed = unnamed.unwrap_or_else(|| panic!("2, {transfer}"));
            assert_eq!(converted(&unnamed), (false, COLOURS.to_vec()), "{transfer}");
        }
        for (primaries, transfer) in [
            (9, 16),
            (9, 18),
            (12, 16),
            (2, 16),
            (0, 13),
            (300, 13),
            // XYZ is no space a photo is in; nor are these two curves a photo's.
            (10, 13),
            (12, 17),
            (12, 300),
        ] {
            assert!(
                embedded_profile(&avif_saying(dir.path(), primaries, transfer)).is_none(),
                "{primaries}, {transfer}"
            );
        }
    }

    // An AVIF can carry an ICC profile instead, which is read as any other file's.
    #[test]
    fn an_avifs_icc_profile_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = avif_fixture("icc_709_limited.avif");
        let path = dir.path().join("a.avif");
        std::fs::write(&path, &bytes).unwrap();
        let own = embedded_profile(&path).expect("the fixture's own profile");
        // (It is an sRGB profile: nothing to convert.)
        assert_eq!(converted(&own), (false, COLOURS.to_vec()));
        // A profile cut short in the box is no profile.
        let at = (bytes.windows(8))
            .position(|window| window == b"colrprof" || window == b"colrrICC")
            .expect("the fixture's ICC");
        bytes[at + 8..at + 8 + 128].fill(0);
        std::fs::write(&path, &bytes).unwrap();
        assert!(embedded_profile(&path).is_none());
    }
}

/// Profiles and files written by hand, a byte at a time, for what moxcms's own writer
/// and `image`'s do not make: a curve that is a table, a `cicp` tag, a PNG whose profile
/// inflates. (The writer and most of these tests are a reviewer's.)
#[cfg(test)]
mod by_hand {
    use super::*;
    use crate::testutil::avif_fixture;

    fn s15(v: f64) -> [u8; 4] {
        ((v * 65536.0).round() as i32).to_be_bytes()
    }

    fn xyz_tag(x: f64, y: f64, z: f64) -> Vec<u8> {
        let mut t = b"XYZ \0\0\0\0".to_vec();
        t.extend(s15(x));
        t.extend(s15(y));
        t.extend(s15(z));
        t
    }

    /// `curv` with a table of 16-bit entries; one entry is a gamma in u8Fixed8.
    fn curv(entries: &[u16]) -> Vec<u8> {
        let mut t = b"curv\0\0\0\0".to_vec();
        t.extend((entries.len() as u32).to_be_bytes());
        for e in entries {
            t.extend(e.to_be_bytes());
        }
        t
    }

    /// `para` of function type `kind` with its parameters.
    fn para(kind: u16, params: &[f64]) -> Vec<u8> {
        let mut t = b"para\0\0\0\0".to_vec();
        t.extend(kind.to_be_bytes());
        t.extend([0, 0]);
        for p in params {
            t.extend(s15(*p));
        }
        t
    }

    fn srgb_para() -> Vec<u8> {
        para(3, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045])
    }

    /// sRGB's EOTF.
    fn srgb_eotf(v: f64) -> f64 {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }

    /// A whole profile: header, tag table, tags.
    fn icc(
        version: u32,
        class: &[u8; 4],
        space: &[u8; 4],
        pcs: &[u8; 4],
        tags: &[(&[u8; 4], Vec<u8>)],
    ) -> Vec<u8> {
        let mut out = vec![0u8; 128];
        out[8..12].copy_from_slice(&version.to_be_bytes());
        out[12..16].copy_from_slice(class);
        out[16..20].copy_from_slice(space);
        out[20..24].copy_from_slice(pcs);
        // date: 2026-01-01
        out[24..26].copy_from_slice(&2026u16.to_be_bytes());
        out[26..28].copy_from_slice(&1u16.to_be_bytes());
        out[28..30].copy_from_slice(&1u16.to_be_bytes());
        out[36..40].copy_from_slice(b"acsp");
        out[68..72].copy_from_slice(&s15(0.9642));
        out[72..76].copy_from_slice(&s15(1.0));
        out[76..80].copy_from_slice(&s15(0.8249));
        out.extend((tags.len() as u32).to_be_bytes());
        let table_at = out.len();
        out.resize(table_at + tags.len() * 12, 0);
        for (i, (sig, data)) in tags.iter().enumerate() {
            while !out.len().is_multiple_of(4) {
                out.push(0);
            }
            let at = out.len() as u32;
            out.extend(data);
            let e = table_at + i * 12;
            out[e..e + 4].copy_from_slice(*sig);
            out[e + 4..e + 8].copy_from_slice(&at.to_be_bytes());
            out[e + 8..e + 12].copy_from_slice(&(data.len() as u32).to_be_bytes());
        }
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        let size = out.len() as u32;
        out[0..4].copy_from_slice(&size.to_be_bytes());
        out
    }

    /// sRGB's colorants adapted to D50 (the ones every sRGB profile carries).
    const SRGB_R: (f64, f64, f64) = (0.43607, 0.22249, 0.01392);
    const SRGB_G: (f64, f64, f64) = (0.38515, 0.71687, 0.09708);
    const SRGB_B: (f64, f64, f64) = (0.14307, 0.06061, 0.71410);

    /// A v2 matrix profile with the given colorants and one curve for all three channels.
    fn matrix_profile(
        version: u32,
        r: (f64, f64, f64),
        g: (f64, f64, f64),
        b: (f64, f64, f64),
        trc: Vec<u8>,
    ) -> Vec<u8> {
        icc(
            version,
            b"mntr",
            b"RGB ",
            b"XYZ ",
            &[
                (b"wtpt", xyz_tag(0.9642, 1.0, 0.8249)),
                (b"rXYZ", xyz_tag(r.0, r.1, r.2)),
                (b"gXYZ", xyz_tag(g.0, g.1, g.2)),
                (b"bXYZ", xyz_tag(b.0, b.1, b.2)),
                (b"rTRC", trc.clone()),
                (b"gTRC", trc.clone()),
                (b"bTRC", trc),
            ],
        )
    }

    fn convert(profile: &Profile, pixels: &[[u8; 3]]) -> (bool, Vec<[u8; 3]>) {
        let mut rgba: Vec<u8> = pixels
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect();
        let changed = to_srgb(&mut rgba, profile);
        let pixels = (rgba.as_chunks::<4>().0.iter())
            .map(|p| [p[0], p[1], p[2]])
            .collect();
        (changed, pixels)
    }

    /// Display P3's colorants (D50), as an iPhone's profile carries them.
    const P3_R: (f64, f64, f64) = (0.51512, 0.24120, -0.00105);
    const P3_G: (f64, f64, f64) = (0.29198, 0.69225, 0.04189);
    const P3_B: (f64, f64, f64) = (0.15710, 0.06657, 0.78407);

    /// `cicp` tag: primaries, transfer, matrix, range.
    fn cicp_tag(p: u8, t: u8) -> Vec<u8> {
        let mut v = b"cicp\0\0\0\0".to_vec();
        v.extend([p, t, 0, 1]);
        v
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut table = [0u32; 256];
        for (i, slot) in table.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *slot = c;
        }
        let mut crc = u32::MAX;
        for b in bytes {
            crc = table[((crc ^ u32::from(*b)) & 0xFF) as usize] ^ (crc >> 8);
        }
        !crc
    }

    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        let mut body = kind.to_vec();
        body.extend(data);
        out.extend(&body);
        out.extend(crc32(&body).to_be_bytes());
        out
    }

    /// A zlib stream that inflates to `1 + 258 * matches` zero bytes: one literal, then that
    /// many "the last byte again, 258 times", thirteen bits each.
    fn zeros_deflated(matches: usize) -> (Vec<u8>, usize) {
        let mut out = vec![0x78, 0x01];
        let (mut acc, mut n) = (0u64, 0u32);
        let mut put = |out: &mut Vec<u8>, bits: &[u8]| {
            for bit in bits {
                acc |= u64::from(*bit) << n;
                n += 1;
                if n == 8 {
                    out.push(acc as u8);
                    acc = 0;
                    n = 0;
                }
            }
        };
        put(&mut out, &[1, 1, 0]); // last block, fixed codes
        put(&mut out, &[0, 0, 1, 1, 0, 0, 0, 0]); // the literal 0
        for _ in 0..matches {
            put(&mut out, &[1, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0]); // length 258, distance 1
        }
        put(&mut out, &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // end of block, and padding
        let len = 1 + 258 * matches;
        let adler = (((len % 65521) as u32) << 16) | 1;
        out.extend(adler.to_be_bytes());
        (out, len)
    }

    fn png_with_iccp_inflating_to(matches: usize) -> (Vec<u8>, usize) {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]));
        let (stream, len) = zeros_deflated(matches);
        let mut iccp = b"x\0\0".to_vec();
        iccp.extend(stream);
        png.extend(chunk(b"iCCP", &iccp));
        // one pixel, stored: filter 0, then (200, 100, 50)
        let raw = [0u8, 200, 100, 50];
        let (mut a, mut b) = (1u32, 0u32);
        for v in raw {
            a = (a + u32::from(v)) % 65521;
            b = (b + a) % 65521;
        }
        let mut idat = vec![0x78, 0x01, 0x01, 4, 0, 0xFB, 0xFF];
        idat.extend(raw);
        idat.extend(((b << 16) | a).to_be_bytes());
        png.extend(chunk(b"IDAT", &idat));
        png.extend(chunk(b"IEND", &[]));
        (png, len)
    }

    /// sRGB's curve as a `curv` table of `n` entries, with `bend` applied to each (input, linear).
    fn srgb_table(n: usize, bend: impl Fn(f64, f64) -> f64) -> Vec<u8> {
        let entries: Vec<u16> = (0..n)
            .map(|i| {
                let v = i as f64 / (n - 1) as f64;
                (bend(v, srgb_eotf(v)).clamp(0.0, 1.0) * 65535.0).round() as u16
            })
            .collect();
        curv(&entries)
    }

    // sRGB's primaries, and a curve that is a table: sRGB's own at six steps (0, 51, ...
    // 255) and lifted in the shadows between the first two, as a display's measured curve
    // with a raised black is. Tried on six steps of each channel, as the spread first was,
    // it was "sRGB already" with every grey from 1 to 50 out, by up to seventeen steps.
    #[test]
    fn a_profile_that_differs_from_srgb_only_between_a_few_steps_is_still_converted() {
        // lifted shadows: a bump over (0, 0.2), nought at both ends
        let lifted = srgb_table(1024, |v, linear| {
            if v < 0.2 {
                linear + 0.012 * (std::f64::consts::PI * v / 0.2).sin()
            } else {
                linear
            }
        });
        let icc = matrix_profile(0x0240_0000, SRGB_R, SRGB_G, SRGB_B, lifted);
        let profile = Profile::from_icc(&icc).expect("applied");
        // what the conversion itself does to a grey ramp, asked of moxcms directly
        let srgb = ColorProfile::new_srgb();
        let conversion = (profile.0)
            .create_transform_8bit(
                Layout::Rgba,
                &srgb,
                Layout::Rgba,
                TransformOptions::default(),
            )
            .unwrap();
        let ramp: Vec<u8> = (0..=255u8).flat_map(|v| [v, v, v, 255]).collect();
        let mut moved = vec![0u8; ramp.len()];
        conversion.transform(&ramp, &mut moved).unwrap();
        let worst = (ramp.chunks(4).zip(moved.chunks(4)))
            .map(|(a, b)| a[0].abs_diff(b[0]))
            .max()
            .unwrap();
        let how_many = (ramp.chunks(4).zip(moved.chunks(4)))
            .filter(|(a, b)| a[0].abs_diff(b[0]) > 1)
            .count();
        assert!(
            worst > 10,
            "the fixture is meant to differ visibly: {worst}"
        );
        // and what to_srgb makes of a picture of those greys
        let mut picture = ramp.clone();
        let changed = to_srgb(&mut picture, &profile);
        assert!(
            changed,
            "a profile that moves {how_many} greys by up to {worst} steps was taken for sRGB"
        );
    }

    // A profile with sRGB's primaries, sRGB's curve in its curve tags, and a `cicp` tag
    // (ICC 4.4) that says PQ or HLG. By moxcms's default the curve is taken from that
    // tag, and the picture was converted by PQ's: mid grey to near black. High dynamic
    // range is left as it is, by whichever door it comes: the curves are the curve tags'.
    #[test]
    fn a_profiles_curves_are_its_curve_tags_whatever_its_cicp_tag_says() {
        let with = |cicp: Option<(u8, u8)>| {
            let mut tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
                (b"wtpt", xyz_tag(0.9642, 1.0, 0.8249)),
                (b"rXYZ", xyz_tag(SRGB_R.0, SRGB_R.1, SRGB_R.2)),
                (b"gXYZ", xyz_tag(SRGB_G.0, SRGB_G.1, SRGB_G.2)),
                (b"bXYZ", xyz_tag(SRGB_B.0, SRGB_B.1, SRGB_B.2)),
                (b"rTRC", srgb_para()),
                (b"gTRC", srgb_para()),
                (b"bTRC", srgb_para()),
            ];
            if let Some((p, t)) = cicp {
                tags.push((b"cicp", cicp_tag(p, t)));
            }
            icc(0x0440_0000, b"mntr", b"RGB ", b"XYZ ", &tags)
        };
        let grey = [[128u8, 128, 128], [64, 64, 64]];
        let plain = convert(&Profile::from_icc(&with(None)).unwrap(), &grey);
        assert_eq!(plain, (false, grey.to_vec()), "without the tag it is sRGB");
        for (what, cicp) in [("PQ", (1, 16)), ("HLG", (1, 18))] {
            let (changed, out) = convert(&Profile::from_icc(&with(Some(cicp))).unwrap(), &grey);
            assert!(
                !changed,
                "{what}: (128, 128, 128) and (64, 64, 64) came out as {out:?}"
            );
        }
    }

    // And a profile whose `cicp` tag says BT.709 is read as the same words in an AVIF's
    // box are (`from_cicp`): by sRGB's curve, which is what its curve tags hold, and not
    // by BT.709's own formula.
    #[test]
    fn bt709_said_by_a_profiles_cicp_tag_is_read_as_bt709_said_by_an_avifs_numbers() {
        let mut tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
            (b"wtpt", xyz_tag(0.9642, 1.0, 0.8249)),
            (b"rXYZ", xyz_tag(P3_R.0, P3_R.1, P3_R.2)),
            (b"gXYZ", xyz_tag(P3_G.0, P3_G.1, P3_G.2)),
            (b"bXYZ", xyz_tag(P3_B.0, P3_B.1, P3_B.2)),
            (b"rTRC", srgb_para()),
            (b"gTRC", srgb_para()),
            (b"bTRC", srgb_para()),
        ];
        tags.push((b"cicp", cicp_tag(12, 1)));
        let by_profile =
            Profile::from_icc(&icc(0x0440_0000, b"mntr", b"RGB ", b"XYZ ", &tags)).unwrap();
        let by_numbers = Profile::from_cicp(12, 1).unwrap();
        let colours = [[128u8, 128, 128], [200, 100, 50]];
        assert_eq!(
            convert(&by_profile, &colours),
            convert(&by_numbers, &colours)
        );
    }

    // Primaries that are not said, with a curve that is: each of the two has a default
    // of its own (BT.709's primaries, sRGB's curve - MIAF 7.3.6.4), so linear light with
    // no primaries named is linear BT.709, and mid grey 128 is sRGB's 188. Left as it
    // was for want of primaries, it was shown far too dark.
    #[test]
    fn an_avif_with_no_primaries_named_and_a_curve_that_is_has_bt709s() {
        let grey = [[128u8, 128, 128]];
        let named = convert(&Profile::from_cicp(1, 8).expect("linear BT.709"), &grey);
        assert_eq!(named, (true, vec![[188, 188, 188]]));
        for transfer in [8, 4, 5] {
            let unnamed = Profile::from_cicp(2, transfer);
            let unnamed = unnamed.unwrap_or_else(|| panic!("transfer {transfer}: left as it is"));
            let named = Profile::from_cicp(1, transfer).unwrap();
            assert_eq!(
                convert(&unnamed, &grey),
                convert(&named, &grey),
                "{transfer}"
            );
        }
    }

    // A PNG's profile is deflated, and a few hundred kilobytes of file can say "a
    // profile of thirty megabytes" (or of thirty gigabytes). No profile is that large:
    // it is refused unread. Read with no limit - which is what `PngDecoder::new` is -
    // a file of four megabytes took 1.3 GB to say it had no profile.
    #[test]
    fn a_pngs_profile_is_not_inflated_without_a_bound() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.png");
        let (png, inflated) = png_with_iccp_inflating_to(32 * 1024 * 1024 / 258);
        assert!(inflated > 30 * 1024 * 1024 && png.len() < 400 * 1024);
        std::fs::write(&path, &png).unwrap();
        assert!(
            said(&path).is_none(),
            "thirty megabytes of profile were read"
        );
        // The picture itself is one photon shows: a pixel of the colour it says.
        let picture = crate::decode::decode_image(&path).expect("decodes");
        assert_eq!(picture.to_rgb8().get_pixel(0, 0).0, [200, 100, 50]);
        // And a profile of a size profiles are is read, by the same road.
        let (png, inflated) = png_with_iccp_inflating_to(2 * 1024 * 1024 / 258);
        std::fs::write(&path, &png).unwrap();
        match said(&path) {
            Some(avif::Colours::Icc(icc)) => assert_eq!(icc.len(), inflated),
            _ => panic!("two megabytes of profile were not read"),
        }
    }

    // The plain curves an AVIF can name - a gamma of 2.2, one of 2.8, and none at all -
    // each by its own arithmetic, worked out by hand for three greys under BT.709's
    // primaries, which are sRGB's.
    #[test]
    fn an_avifs_plain_gamma_and_its_linear_light_are_converted_by_their_own_curves() {
        let greys = [[64u8, 64, 64], [128, 128, 128], [200, 200, 200]];
        for (transfer, want) in [
            (4, [62u8, 129, 201]),
            (5, [40, 106, 189]),
            (8, [137, 188, 229]),
        ] {
            let profile = Profile::from_cicp(1, transfer).expect("a curve photon names");
            let (changed, out) = convert(&profile, &greys);
            let want: Vec<[u8; 3]> = want.iter().map(|v| [*v, *v, *v]).collect();
            assert!(changed, "{transfer}");
            assert_eq!(out, want, "{transfer}");
        }
    }

    // A number an AVIF's box has sixteen bits for and the table eight: one too large is
    // no colour space, and not the one its low eight bits would name (268 is 12, Display
    // P3, with a bit above).
    #[test]
    fn a_number_too_large_for_the_table_names_nothing() {
        assert!(Profile::from_cicp(268, 13).is_none());
        assert!(Profile::from_cicp(12, 269).is_none());
        assert!(Profile::from_cicp(12, 13).is_some());
    }

    // An AVIF whose major brand is the generic `mif1`, with `avif` among the ones it is
    // compatible with, is read as the decoder reads it.
    #[test]
    fn an_avif_under_a_generic_major_brand_says_its_colours_too() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = avif_fixture("red_blue_709_limited.avif");
        let at = (bytes.windows(8))
            .position(|window| window == b"colrnclx")
            .expect("the fixture's nclx");
        bytes[at + 8..at + 10].copy_from_slice(&12u16.to_be_bytes());
        bytes[at + 10..at + 12].copy_from_slice(&13u16.to_be_bytes());
        assert_eq!(&bytes[4..8], b"ftyp");
        assert!(
            bytes[16..64].windows(4).any(|brand| brand == b"avif"),
            "compatible"
        );
        bytes[8..12].copy_from_slice(b"mif1");
        let path = dir.path().join("generic.avif");
        std::fs::write(&path, &bytes).unwrap();
        assert!(crate::decode::decode_image(&path).is_ok(), "it is decoded");
        assert!(embedded_profile(&path).is_some());
    }
}
