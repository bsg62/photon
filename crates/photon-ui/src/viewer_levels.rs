//! The full picture at more than one size. egui's wgpu renderer gives a texture one level
//! (`mip_level_count: 1`, whatever `TextureOptions::mipmap_mode` says), so a 24 MP texture
//! fitted into a window is sampled from a few of its pixels in every ten, and shimmers.
//! photon makes the smaller levels itself: halves, each the mean of four pixels, and the
//! view draws the smallest that is at least as large as what is drawn - so a photo drawn
//! half of `LEVEL_FLOOR` or more on its longer side is never drawn from a texture more
//! than twice its size, where a plain linear filter is enough.

use crate::thumbs::textures::Pixels;

/// Halving stops at the first level whose longer side is no more than this, so the
/// smallest level's longer side is between half of it and all of it: smaller than a
/// photo is fitted into in any window photos are looked at in. It was 2,048, "smaller
/// than any area a photo is fitted into", which was not so: an upright 12 MP photo in a
/// window 700 points high is 525 pixels wide, and was drawn from a level 1,500 wide -
/// until its fitted copy came, and whenever the window changed size. The levels below
/// 2,048 are a sixteenth of the picture and less, all of them together.
pub const LEVEL_FLOOR: u32 = 512;

/// `pixels` at half its size: each pixel the mean of the four it covers. An odd last row
/// or column is left out, which is half a pixel of a picture thousands across.
pub fn halve(pixels: &Pixels) -> Pixels {
    let (width, height) = ((pixels.width / 2).max(1), (pixels.height / 2).max(1));
    let stride = pixels.width as usize * 4;
    // A side of one pixel has no pair to take the mean of: its one pixel twice.
    let (right, below) = (
        if pixels.width > 1 { 4 } else { 0 },
        if pixels.height > 1 { stride } else { 0 },
    );
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height as usize {
        let row = y * 2 * stride;
        for x in 0..width as usize {
            let at = row + x * 2 * 4;
            for channel in 0..4 {
                let sum = u16::from(pixels.rgba[at + channel])
                    + u16::from(pixels.rgba[at + right + channel])
                    + u16::from(pixels.rgba[at + below + channel])
                    + u16::from(pixels.rgba[at + below + right + channel]);
                rgba.push(((sum + 2) / 4) as u8);
            }
        }
    }
    Pixels {
        width,
        height,
        rgba,
    }
}

/// The levels of `full`, largest first: itself and its halves down to the first whose
/// longer side is no more than `LEVEL_FLOOR`, without those a texture cannot hold
/// (`max_side`, the device's limit). The largest left is then the deepest there is to
/// zoom into: on a device that holds 8192 pixels a 12,000-pixel panorama is shown from
/// its half. Never none: the smallest is kept whatever the limit says.
pub fn levels(full: Pixels, max_side: u32) -> Vec<Pixels> {
    let mut levels = vec![full];
    while levels
        .last()
        .is_some_and(|level| level.width.max(level.height) > LEVEL_FLOOR)
    {
        let next = halve(levels.last().expect("there is a level"));
        levels.push(next);
    }
    let fits = |level: &Pixels| level.width.max(level.height) <= max_side;
    let first = (levels.iter().position(fits)).unwrap_or(levels.len() - 1);
    levels.split_off(first)
}

/// Which of the levels, given by their widths largest first, is drawn where the picture
/// is `drawn` pixels wide: the smallest that is at least that wide, so that it is drawn at
/// between half its size and all of it; the largest, where the picture is drawn larger
/// than any of them.
pub fn level_for(widths: &[u32], drawn: f32) -> usize {
    (widths.iter())
        .rposition(|width| *width as f32 + 0.5 >= drawn)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(width: u32, height: u32, rgba: [u8; 4]) -> Pixels {
        Pixels {
            width,
            height,
            rgba: rgba.repeat((width * height) as usize),
        }
    }

    #[test]
    fn a_half_is_half_as_wide_and_high_and_each_pixel_the_mean_of_four() {
        // Two by two blocks: (0, 10, 20, 30) and (100, 100, 100, 103), in red. The
        // second's mean is 100.75, which is 101 to the nearest and 100 cut off.
        let pixels = Pixels {
            width: 4,
            height: 2,
            rgba: [
                [0, 0, 0, 255],
                [10, 0, 0, 255],
                [100, 0, 0, 255],
                [100, 0, 0, 255],
                [20, 0, 0, 255],
                [30, 0, 0, 255],
                [100, 0, 0, 255],
                [103, 0, 0, 255],
            ]
            .concat(),
        };
        let half = halve(&pixels);
        assert_eq!((half.width, half.height), (2, 1));
        assert_eq!(half.rgba, [15, 0, 0, 255, 101, 0, 0, 255]);
    }

    #[test]
    fn an_odd_side_loses_its_last_row_and_a_side_of_one_stays_one() {
        let half = halve(&flat(5, 3, [8, 8, 8, 255]));
        assert_eq!((half.width, half.height), (2, 1));
        assert_eq!(half.rgba.len(), 2 * 4);
        // A strip one pixel high: halved along its length only.
        let strip = halve(&flat(6, 1, [9, 9, 9, 255]));
        assert_eq!((strip.width, strip.height), (3, 1));
        assert_eq!(strip.rgba, [9, 9, 9, 255].repeat(3));
        let dot = halve(&flat(1, 1, [7, 7, 7, 255]));
        assert_eq!((dot.width, dot.height), (1, 1));
    }

    fn sides(levels: &[Pixels]) -> Vec<(u32, u32)> {
        levels
            .iter()
            .map(|level| (level.width, level.height))
            .collect()
    }

    #[test]
    fn the_levels_are_the_picture_and_its_halves_down_to_one_that_fits_any_area() {
        let made = levels(flat(6000, 4000, [1, 2, 3, 255]), 16384);
        assert_eq!(
            sides(&made),
            [
                (6000, 4000),
                (3000, 2000),
                (1500, 1000),
                (750, 500),
                (375, 250)
            ]
        );
        // A picture no larger than the floor is its own one level.
        assert_eq!(sides(&levels(flat(512, 341, [0; 4]), 16384)), [(512, 341)]);
        assert_eq!(levels(flat(513, 341, [0; 4]), 16384).len(), 2);
        // By the longer side, whichever it is.
        assert_eq!(
            sides(&levels(flat(250, 1250, [0; 4]), 16384)),
            [(250, 1250), (125, 625), (62, 312)]
        );
    }

    // What the floor is for: fitted into any window a photo is looked at in, the photo
    // is drawn from a level no more than twice as large as what is drawn. An upright
    // 12 MP photo in a window 700 pixels high, and a panorama in one 1,000 wide.
    #[test]
    fn a_photo_fitted_into_a_window_is_drawn_from_no_more_than_twice_its_size() {
        for (width, height, drawn) in [(3000, 4000, 525.0), (12000, 3000, 1000.0)] {
            let widths: Vec<u32> = (levels(flat(width, height, [0; 4]), 16384).iter())
                .map(|level| level.width)
                .collect();
            let level = widths[level_for(&widths, drawn)] as f32;
            assert!(
                level >= drawn && level <= drawn * 2.0,
                "{drawn} pixels drawn from a level {level} wide, of {widths:?}"
            );
        }
    }

    // The umbrella spec's fifth risk: a texture's side is limited by the device, and the
    // deepest zoom into a very large picture is then the first half that fits.
    #[test]
    fn a_level_the_device_cannot_hold_is_left_out() {
        let made = levels(flat(12000, 3000, [0; 4]), 8192);
        assert_eq!(
            sides(&made),
            [
                (6000, 1500),
                (3000, 750),
                (1500, 375),
                (750, 187),
                (375, 93)
            ]
        );
        // At the limit exactly, it fits.
        assert_eq!(levels(flat(8192, 100, [0; 4]), 8192).len(), 5);
        // A limit below every level leaves the smallest: there is always a picture.
        assert_eq!(sides(&levels(flat(6000, 4000, [0; 4]), 200)), [(375, 250)]);
    }

    #[test]
    fn the_level_drawn_is_the_smallest_at_least_as_wide_as_what_is_drawn() {
        let widths = [6000, 3000, 1500];
        // Fitted into a 2,700-pixel area: the half, at nine tenths of its size.
        assert_eq!(level_for(&widths, 2700.0), 1);
        assert_eq!(level_for(&widths, 1500.0), 2);
        assert_eq!(
            level_for(&widths, 1500.4),
            2,
            "a rounding's worth over is the same"
        );
        assert_eq!(level_for(&widths, 1501.0), 1);
        assert_eq!(level_for(&widths, 3001.0), 0);
        assert_eq!(level_for(&widths, 400.0), 2);
        // Zoomed past its own pixels: the largest there is.
        assert_eq!(level_for(&widths, 9000.0), 0);
        assert_eq!(level_for(&[800], 9000.0), 0);
    }
}
