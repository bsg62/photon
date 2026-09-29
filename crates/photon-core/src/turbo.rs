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
    // The whole lifecycle runs inside the one catch, including the drop that destroys
    // libjpeg's state. An unwind out of any of it is a file handed back, never a panic that
    // reaches the thumbnail service's own `catch_unwind` as a decoder bug.
    catch_unwind(AssertUnwindSafe(|| decode(bytes, target)))
        .ok()
        .flatten()
}

fn decode(bytes: &[u8], target: (u32, u32)) -> Option<DynamicImage> {
    // This decode must not save markers (`with_markers`) or crop or skip scanlines: the
    // vendored mozjpeg 4.1.5 lacks upstream libjpeg-turbo's security fixes for exactly those
    // paths. See the spec's "Build" section ("Security fixes").
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
        assert!(
            scaled_decode(&arithmetic, (1600, 19)).is_none(),
            "arithmetic"
        );

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
