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

use std::collections::HashMap;

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

/// The largest distance at which grouping has **complete** recall.
///
/// Candidate pairs are found by splitting each hash into four 16-bit bands and bucketing by
/// band. Two hashes within Hamming distance 3 must agree exactly on at least one band -
/// four bands, at most three differing bits, so some band has none. That makes the buckets
/// an exact filter at this distance, not a heuristic. Above it, a pair is found only if it
/// happens to share a band, which is why the UI says "finds most" rather than "finds all".
pub const EXACT_RECALL_DISTANCE: u32 = 3;

const BANDS: u32 = 4;
const BAND_BITS: u32 = 16;

/// Groups look-alikes, returning `(item_id, group_id)` for every photo that has at least one.
///
/// `group_id` is the smallest item id in the group, so a group has a stable name that does
/// not depend on iteration order. A photo that resembles nothing is absent from the result
/// rather than present with a NULL - the caller clears the column wholesale first.
///
/// `distance` of 0 turns the feature off and returns nothing.
///
/// Whole-library, not incremental: a union-find over 100k rows is milliseconds, and the
/// incremental version has to reason about a group *splitting* when a photo is purged.
pub fn group(hashes: &[(i64, u64)], distance: u32) -> Vec<(i64, i64)> {
    if distance == 0 || hashes.len() < 2 {
        return Vec::new();
    }

    // Bucket by band. The key mixes the band's index in, so the same 16 bits in two
    // different bands do not collide into one bucket.
    let mut buckets: HashMap<(u32, u16), Vec<usize>> = HashMap::new();
    for (index, (_, hash)) in hashes.iter().enumerate() {
        for band in 0..BANDS {
            let shift = band * BAND_BITS;
            let key = ((hash >> shift) & 0xffff) as u16;
            buckets.entry((band, key)).or_default().push(index);
        }
    }

    let mut parent: Vec<usize> = (0..hashes.len()).collect();
    for indexes in buckets.values() {
        for (i, &a) in indexes.iter().enumerate() {
            for &b in &indexes[i + 1..] {
                if (hashes[a].1 ^ hashes[b].1).count_ones() <= distance {
                    union(&mut parent, a, b);
                }
            }
        }
    }

    // Each root takes the smallest item id beneath it; then every member that shares a root
    // with someone else takes that id.
    let mut smallest: HashMap<usize, i64> = HashMap::new();
    let mut members: HashMap<usize, u32> = HashMap::new();
    for (index, (id, _)) in hashes.iter().enumerate() {
        let root = find(&mut parent, index);
        smallest
            .entry(root)
            .and_modify(|s| *s = (*s).min(*id))
            .or_insert(*id);
        *members.entry(root).or_insert(0) += 1;
    }

    let mut out = Vec::new();
    for (index, (id, _)) in hashes.iter().enumerate() {
        let root = find(&mut parent, index);
        if members[&root] > 1 {
            out.push((*id, smallest[&root]));
        }
    }
    out
}

fn find(parent: &mut [usize], mut node: usize) -> usize {
    while parent[node] != node {
        parent[node] = parent[parent[node]];
        node = parent[node];
    }
    node
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        parent[ra] = rb;
    }
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

    #[test]
    fn a_photo_with_no_look_alike_is_not_in_any_group() {
        let out = group(&[(1, 0x0000_0000_0000_0000), (2, 0xffff_ffff_ffff_ffff)], 3);
        assert!(out.is_empty());
    }

    #[test]
    fn a_close_pair_shares_the_smaller_id_as_its_group() {
        // One bit apart.
        let out = group(&[(7, 0b1010), (4, 0b1011)], 3);
        let mut out = out;
        out.sort();
        assert_eq!(out, vec![(4, 4), (7, 4)]);
    }

    #[test]
    fn groups_are_transitive() {
        // A~B and B~C at distance 3 each, A~C at 6 - all three must land in one group.
        let a = 0u64;
        let b = 0b111u64;
        let c = 0b111_111u64;
        let mut out = group(&[(1, a), (2, b), (3, c)], 3);
        out.sort();
        assert_eq!(out, vec![(1, 1), (2, 1), (3, 1)]);
    }

    /// The pigeonhole argument, made a property of the code rather than of a comment:
    /// four 16-bit bands, so two hashes within distance 3 must agree exactly on some band.
    /// Brute-force every pair and assert the banded filter found all of them.
    #[test]
    fn banding_finds_every_pair_within_the_exact_recall_distance() {
        let mut hashes: Vec<(i64, u64)> = Vec::new();
        let base = 0x0123_4567_89ab_cdefu64;
        for i in 0..64i64 {
            // Flip up to three scattered bits, so pairs land at a range of small distances.
            let h =
                base ^ (1u64 << (i % 64)) ^ (1u64 << ((i * 7) % 64)) ^ (1u64 << ((i * 13) % 64));
            hashes.push((i + 1, h));
        }
        let grouped = group(&hashes, EXACT_RECALL_DISTANCE);
        let in_a_group: std::collections::HashSet<i64> =
            grouped.iter().map(|(id, _)| *id).collect();

        for (ia, ha) in &hashes {
            for (ib, hb) in &hashes {
                if ia >= ib {
                    continue;
                }
                if (ha ^ hb).count_ones() <= EXACT_RECALL_DISTANCE {
                    assert!(
                        in_a_group.contains(ia) && in_a_group.contains(ib),
                        "pair ({ia}, {ib}) at distance {} was missed",
                        (ha ^ hb).count_ones()
                    );
                }
            }
        }
    }

    #[test]
    fn distance_zero_groups_nothing() {
        let out = group(&[(1, 5), (2, 5)], 0);
        assert!(out.is_empty(), "distance 0 means the feature is off");
    }
}
