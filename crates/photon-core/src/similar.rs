//! Finding photos that are the same picture without being the same bytes.
//!
//! `duplicates.rs` answers "are these the same file"; this answers "are these the same
//! photo". A re-saved JPEG, an emailed copy at 2048px and a re-export from another program
//! are all different files with different byte hashes, and the README has always admitted
//! photon could not see them.
//!
//! The hash is a 64-bit **difference hash**: reduce the picture to 9x8 greyscale and set one
//! bit per horizontal neighbour pair according to which is brighter. It is the cheapest hash
//! with the property that matters - invariant to scale and to re-compression, because it
//! records the *relationships* between regions rather than their values. It is deliberately
//! not invariant to a mirror or a rotation: those are different photographs to a person
//! looking for a duplicate.

use image::{DynamicImage, imageops::FilterType};

/// Width of the reduced image. One more than the 8 columns of bits, because each bit
/// compares a pixel with its right-hand neighbour.
const REDUCED_W: u32 = 9;
const REDUCED_H: u32 = 8;

/// A 64-bit difference hash of `img`.
///
/// The reduction does the work: at 9x8 greyscale, JPEG ringing, a WebP re-encode and a
/// resize all vanish, while the arrangement of light and dark survives. `FilterType::Triangle`
/// matches what `decode_oriented` already uses, so a photo reduced here and a photo reduced
/// on the way to a thumbnail agree.
pub fn dhash(img: &DynamicImage) -> u64 {
    let small = img
        .resize_exact(REDUCED_W, REDUCED_H, FilterType::Triangle)
        .to_luma8();
    let mut hash = 0u64;
    for y in 0..REDUCED_H {
        for x in 0..(REDUCED_W - 1) {
            let left = small.get_pixel(x, y).0[0];
            let right = small.get_pixel(x + 1, y).0[0];
            hash = (hash << 1) | u64::from(left > right);
        }
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgb, RgbImage};

    /// A gradient with a bright blob, at whatever size is asked for. The same picture at two
    /// resolutions must hash to (nearly) the same value - that is the whole property.
    fn picture(w: u32, h: u32) -> DynamicImage {
        let mut img = RgbImage::new(w, h);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let fx = x as f32 / w as f32;
            let fy = y as f32 / h as f32;
            let blob = if (fx - 0.3).abs() < 0.12 && (fy - 0.6).abs() < 0.12 {
                90.0
            } else {
                0.0
            };
            let v = (fx * 160.0 + fy * 60.0 + blob).min(255.0) as u8;
            *px = Rgb([v, v.wrapping_add(20), 255 - v]);
        }
        DynamicImage::ImageRgb8(img)
    }

    fn distance(a: u64, b: u64) -> u32 {
        (a ^ b).count_ones()
    }

    #[test]
    fn the_same_picture_at_a_quarter_size_hashes_the_same() {
        let big = dhash(&picture(800, 600));
        let small = dhash(&picture(200, 150));
        assert!(
            distance(big, small) <= 3,
            "distance {} between the same picture at two sizes",
            distance(big, small)
        );
    }

    #[test]
    fn a_mirrored_picture_is_not_a_look_alike() {
        let normal = dhash(&picture(400, 300));
        let flipped = dhash(&picture(400, 300).fliph());
        assert!(
            distance(normal, flipped) > 6,
            "a mirror image hashed within {} of the original",
            distance(normal, flipped)
        );
    }

    /// Noise laid down in blocks the same size as the reduction grid, not per pixel. A
    /// per-pixel ramp mod 256 looked "noisy" but was high-frequency: `resize_exact` to 9x8
    /// is a box-style average over a ~44x37 pixel window, which averages away several full
    /// periods of that ramp to a value within a point or two of 123 everywhere, at every
    /// spot in the image. Both this "noise" image and the gradient then reduced to the same
    /// near-flat picture and hashed within 3 bits - not because the hash conflates unrelated
    /// photos, but because that fixture had no structure left at the scale the hash reduces
    /// to. Blocks aligned to the reduction grid survive the average intact.
    fn unrelated_pattern(w: u32, h: u32) -> DynamicImage {
        let mut img = RgbImage::new(w, h);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let bx = x * REDUCED_W / w;
            let by = y * REDUCED_H / h;
            let v = ((bx * 73 + by * 151 + 41) % 256) as u8;
            *px = Rgb([v, 255 - v, v / 2]);
        }
        DynamicImage::ImageRgb8(img)
    }

    #[test]
    fn two_unrelated_pictures_are_far_apart() {
        let gradient = dhash(&picture(400, 300));
        let other = dhash(&unrelated_pattern(400, 300));
        assert!(
            distance(gradient, other) > 6,
            "unrelated pictures hashed within {}",
            distance(gradient, other)
        );
    }

    /// The bit that would catch a hash that is accidentally constant, which every other
    /// test here would pass.
    #[test]
    fn a_flat_image_and_a_gradient_do_not_share_a_hash() {
        let flat = DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([128, 128, 128])));
        assert_ne!(dhash(&flat), dhash(&picture(64, 64)));
    }

    /// A flat region has no left-brighter-than-right relationship anywhere, so it must hash
    /// to all zero bits. `assert_ne` above only says a flat image differs from *this*
    /// gradient - it stays true whichever way a `>` mistakenly written as `>=` flips a flat
    /// region's bits, since the mutated hash (all ones) still isn't the gradient's. This
    /// pins the actual value, which is what a stray `>=` breaks.
    #[test]
    fn a_flat_image_hashes_to_all_zero_bits() {
        let flat = DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([128, 128, 128])));
        assert_eq!(dhash(&flat), 0);
    }
}
