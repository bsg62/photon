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
    /// conversion of numbers, and is left as it is. Not "unspecified" primaries, which
    /// are taken for sRGB's as a web view takes them. And the curves of BT.709's family
    /// (1, 6, 14, 15), like no curve said at all (2), are taken for sRGB's own, again
    /// as a web view takes them: by its own formula a photo that says "BT.709" comes
    /// out darker than every browser shows it. Beside those, the two plain gammas
    /// (2.2 and 2.8) and a linear one.
    fn from_cicp(primaries: u16, transfer: u16) -> Option<Self> {
        let color_primaries = match CicpColorPrimaries::try_from(u8::try_from(primaries).ok()?) {
            // XYZ is no colour space a photo is in.
            Ok(CicpColorPrimaries::Xyz) | Err(_) => return None,
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
/// At this size the copy stays in the processor's cache, and the whole is as fast as one
/// pass over two pictures (measured: 30 ms for 24 million pixels either way).
const CHUNK_PIXELS: usize = 65_536;

/// The steps of each channel in the spread of colours a profile is tried on.
const SPREAD: [u8; 6] = [0, 51, 102, 153, 204, 255];

/// What `path`'s file says of its colours, where it says something photon applies: an
/// RGB profile, or an AVIF's standard-range colour space. None for a file that says
/// nothing - it is taken for sRGB, as a web view takes it - and for one that cannot be
/// read, whose profile is broken, or whose profile is of another kind of colour: a CMYK
/// or a grey one describes numbers the decoder has already made RGB of by a rule of its
/// own, and no longer describes them.
///
/// A read of its own, after the decode and not part of it: every full decode goes
/// through `decode::decode_image`, and its other callers want no profile.
pub fn embedded_profile(path: &Path) -> Option<Profile> {
    let mut reader = BufReader::new(File::open(path).ok()?);
    // By the file's bytes, as `decode_image` tells an AVIF from the rest.
    if avif::is_avif(reader.fill_buf().ok()?) {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).ok()?;
        return match avif::colour_information(&bytes)? {
            avif::Colours::Icc(icc) => Profile::from_icc(&icc),
            avif::Colours::Named {
                primaries,
                transfer,
            } => Profile::from_cicp(primaries, transfer),
        };
    }
    // (By its bytes alone: a file whose bytes do not say what it is is none of the four
    // formats that carry a profile.)
    let image = ImageReader::new(reader).with_guessed_format().ok()?;
    let format = image.format()?;
    let reader = image.into_inner();
    // Each format's own decoder, made directly, for the four that carry a profile.
    // Through `ImageReader::into_decoder`, which would save the list, a TIFF's profile
    // came back as none - by this module's test, with the decoder made here giving the
    // same file's profile whole.
    let icc = match format {
        ImageFormat::Jpeg => (JpegDecoder::new(reader).ok()?.icc_profile()).ok()??,
        ImageFormat::Png => (PngDecoder::new(reader).ok()?.icc_profile()).ok()??,
        ImageFormat::Tiff => (TiffDecoder::new(reader).ok()?.icc_profile()).ok()??,
        ImageFormat::WebP => (WebPDecoder::new(reader).ok()?.icc_profile()).ok()??,
        _ => return None,
    };
    Profile::from_icc(&icc)
}

/// Converts straight RGBA pixels from `profile`'s colours to sRGB, in place, the alpha
/// left as it is. Answers whether anything was changed: a profile that is sRGB in
/// effect - one under which a spread of colours comes out where it went in, to within a
/// step - is not applied, and one moxcms will not make a conversion from is not either.
pub fn to_srgb(rgba: &mut [u8], profile: &Profile) -> bool {
    let srgb = ColorProfile::new_srgb();
    let options = TransformOptions::default();
    let Ok(conversion) =
        (profile.0).create_transform_8bit(Layout::Rgba, &srgb, Layout::Rgba, options)
    else {
        return false;
    };
    // A spread of colours first: where none of them moves, none of the picture's would.
    // This is how a profile that is sRGB is told - by what it does, whatever it is
    // called and however it is written - and it is nearly every profile there is.
    let spread: Vec<u8> = (SPREAD.iter())
        .flat_map(|r| SPREAD.iter().map(move |g| (*r, *g)))
        .flat_map(|(r, g)| SPREAD.iter().map(move |b| [r, g, *b, 255]))
        .flatten()
        .collect();
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
    // a conversion of numbers: it is left as it is. So is an AVIF that names no
    // primaries, or ones nobody knows.
    #[test]
    fn an_avif_of_high_dynamic_range_or_of_no_named_primaries_is_left_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        for (primaries, transfer) in [
            (9, 16),
            (9, 18),
            (12, 16),
            (2, 13),
            (2, 2),
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
