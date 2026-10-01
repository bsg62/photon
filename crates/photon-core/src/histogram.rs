//! A photo's tonal histogram: how much of the picture sits at each brightness, shadows on
//! the left and highlights on the right. What the viewer's info panel draws.
//!
//! Counted from the photo's cached grid thumbnail, as the look-alike hash is
//! (`photon_core::similar`): the thumbnail is the photo *as shown*, edit included, it is
//! already on disk, and a distribution over 256 pixels' width is the distribution of the
//! photo to well within what a 64-column drawing can show. Decoding the source for it would
//! cost a second a photo.

use image::DynamicImage;

/// How many brightness steps the histogram has. Enough to show a curve's shape at the
/// panel's width, few enough that a thumbnail's pixels fill every one without combing.
pub const BINS: usize = 64;

/// The count of pixels at each of [`BINS`] brightness steps, darkest first.
///
/// Brightness is Rec. 601 luma of the stored values (299 R + 587 G + 114 B), what every
/// camera's own histogram plots; no gamma is undone, so the left edge is what looks black.
/// A fully transparent pixel is not counted: it has no brightness, and a logo on a clear
/// ground would otherwise read as mostly black.
pub fn luminance(img: &DynamicImage) -> [u32; BINS] {
    let mut bins = [0u32; BINS];
    for px in img.to_rgba8().pixels() {
        let [r, g, b, a] = px.0;
        if a == 0 {
            continue;
        }
        let luma = (299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b)) / 1000;
        bins[luma as usize * BINS / 256] += 1;
    }
    bins
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn of(pixels: &[[u8; 4]]) -> [u32; BINS] {
        let mut img = RgbaImage::new(pixels.len() as u32, 1);
        for (x, px) in pixels.iter().enumerate() {
            img.put_pixel(x as u32, 0, Rgba(*px));
        }
        luminance(&DynamicImage::ImageRgba8(img))
    }

    #[test]
    fn black_is_the_first_step_and_white_the_last() {
        let bins = of(&[[0, 0, 0, 255], [255, 255, 255, 255], [255, 255, 255, 255]]);
        assert_eq!(bins[0], 1);
        assert_eq!(bins[BINS - 1], 2);
        assert_eq!(bins.iter().sum::<u32>(), 3);
    }

    #[test]
    fn every_pixel_lands_in_the_step_its_grey_names() {
        // Four greys to a step: 0-3 in the first, 4-7 in the second.
        let bins = of(&[[3, 3, 3, 255], [4, 4, 4, 255], [128, 128, 128, 255]]);
        assert_eq!((bins[0], bins[1], bins[32]), (1, 1, 1));
    }

    #[test]
    fn green_counts_for_more_than_red_and_red_for_more_than_blue() {
        // Pure primaries are far apart in brightness: 29, 76 and 149 of 255.
        let bins = of(&[[0, 0, 255, 255], [255, 0, 0, 255], [0, 255, 0, 255]]);
        assert_eq!((bins[7], bins[19], bins[37]), (1, 1, 1));
    }

    #[test]
    fn a_clear_pixel_is_not_counted_and_a_faint_one_is() {
        let bins = of(&[[0, 0, 0, 0], [0, 0, 0, 1]]);
        assert_eq!(bins.iter().sum::<u32>(), 1);
    }
}
