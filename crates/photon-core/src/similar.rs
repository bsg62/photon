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
use std::sync::atomic::{AtomicBool, Ordering};

use image::{DynamicImage, imageops::FilterType};

use crate::{
    Result,
    library::Library,
    thumbs::{ThumbCache, ThumbSize},
};

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
        if pairs_with_anything(*hash, distance) {
            continue;
        }
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

/// Whether `hash` would match another hash *whatever picture that one came from*.
///
/// `popcount(a ^ b) <= popcount(a) + popcount(b)`, so two hashes are within `distance` of
/// each other for certain - regardless of what either photograph was - as soon as
/// `popcount(a) + popcount(b) <= distance`. The single-hash form of that is this: a hash
/// with `2 * popcount(h) <= distance` pairs with anything else that empty, and the same
/// holds at the other end, where almost every bit is set.
///
/// A photo like that is compared with nothing and appears in no group. The worked case is
/// the flat picture, whose hash is exactly 0 - no pixel is brighter than its right-hand
/// neighbour anywhere - so every lens-cap shot, blank scan and near-black frame in a
/// library is distance 0 from every other, and one union-find group would swallow the lot.
/// The user would be shown twenty unrelated dark frames as one set of look-alikes and
/// asked which to delete: the opposite of the "never waste your time" this view promises.
///
/// **This is not a popcount floor**, and turning it into one would throw away real matches.
/// Two hashes at popcount 2 can be 4 bits apart, so they pair on their pictures rather than
/// on their emptiness, and at a loose setting that pairing is a true one. The rule excludes
/// only the hashes whose matching carries no information at the distance being asked for -
/// which is why it takes `distance` rather than testing the hash alone.
fn pairs_with_anything(hash: u64, distance: u32) -> bool {
    2 * hash.count_ones() <= distance || 2 * (!hash).count_ones() <= distance
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

/// What one pass did; see [`update`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PassOutcome {
    /// How many rows took a new hash.
    pub hashed: u64,
    /// Whether the regroup changed which photos are in which group. Separate from `hashed`
    /// because the two come apart: changing the distance setting regroups the whole library
    /// having hashed nothing, and a photo purged out of a group does the same.
    pub groups_changed: bool,
}

/// Hashes every photo that has a thumbnail but no hash, then regroups the whole library.
///
/// The hash comes from the **cached 256px grid thumbnail**, not from the photo: the
/// thumbnail is the reduced image this hash wants, it is already on disk, and decoding it
/// costs about a millisecond against the ~175ms a source decode costs. That is also what
/// makes an existing library fill in - the thumbnail renderer skips a photo whose thumbnail
/// is already cached, so a hash computed there would never have been computed at all for
/// any photo indexed before this feature existed, which is every photo in every library.
///
/// A thumbnail that cannot be read is skipped and the row stays a candidate: it is usually
/// a cache still being written, and the next scan's pass tries again. Cancelling stops
/// between photos; what was hashed so far is kept.
pub fn update(
    lib: &Library,
    cache: &ThumbCache,
    distance: u32,
    cancel: &AtomicBool,
) -> Result<PassOutcome> {
    let mut hashed = 0;
    for candidate in lib.similar_candidates()? {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let path = cache.path_for(candidate.thumb_key, ThumbSize::Grid);
        match image::open(&path) {
            // False only when the row moved on while the thumbnail was being read, which is
            // the row no longer being the one hashed rather than a failure.
            Ok(img) => {
                if lib.set_percep_hash(&candidate, dhash(&img))? {
                    hashed += 1;
                }
            }
            Err(err) => {
                tracing::debug!(id = candidate.id, %err, "could not read a thumbnail to hash");
            }
        }
    }

    // Regroup unconditionally, not only when something was hashed: the distance setting may
    // have changed, or a photo may have been purged out of a group since the last pass.
    let groups_changed = lib.set_similar_groups(&group(&lib.percep_hashes()?, distance))?;
    Ok(PassOutcome {
        hashed,
        groups_changed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::NewItem;
    use crate::media::{ThumbState, fingerprint};
    use crate::testutil::{encode, new_item, seed_folder, temp_library, write_file};
    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
    use std::path::Path;

    /// `new_item` fixes size and mtime; these tests turn them into thumbnail cache keys, so
    /// they name them rather than relying on the shared helper's values.
    fn item_at(folder: i64, path: &str, size: i64, mtime_ms: i64) -> NewItem {
        NewItem {
            size,
            mtime_ms,
            ..new_item(folder, path, 0)
        }
    }

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
        // Two hashes as far apart as two hashes get, both with plenty of structure - an
        // empty pair would be left out by `pairs_with_anything` before the distance was
        // ever measured, and this test is about the distance.
        let out = group(&[(1, 0xffff_ffff_0000_0000), (2, 0x0000_0000_ffff_ffff)], 3);
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
        // Built on a base with structure rather than on zero, which `pairs_with_anything`
        // drops before any of them is compared.
        let a = 0xffff_0000u64;
        let b = a ^ 0b111u64;
        let c = a ^ 0b111_111u64;
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

    /// Every flat picture hashes to the same value, so they are all distance 0 from each
    /// other: without the skip, one group swallows every lens-cap shot, blank scan and
    /// near-black frame in the library, and the user is asked which of twenty unrelated
    /// dark frames to delete.
    #[test]
    fn a_picture_with_no_structure_is_a_look_alike_of_nothing() {
        let mut out = group(
            &[
                (1, dhash(&picture(400, 300))),
                (2, dhash(&picture(100, 75))),
                (3, 0),        // a lens cap
                (4, 0),        // and another, from a different day
                (5, u64::MAX), // the same emptiness, with the comparison read the other way
                (6, u64::MAX),
            ],
            EXACT_RECALL_DISTANCE,
        );
        out.sort();
        assert_eq!(
            out,
            vec![(1, 1), (2, 1)],
            "a picture with no structure was grouped"
        );
    }

    /// The case an exact `0 || u64::MAX` test misses. Two hashes one bit from empty are two
    /// bits apart, so at distance 3 they pair on their emptiness and on nothing else - and
    /// a third, genuinely structured photo one bit from either of them would be dragged in
    /// with them. `2 * popcount <= distance` is what catches the class rather than its
    /// bottom value.
    #[test]
    fn two_almost_empty_pictures_do_not_pair_on_their_emptiness() {
        let out = group(&[(1, 1 << 3), (2, 1 << 40)], EXACT_RECALL_DISTANCE);
        assert!(out.is_empty(), "two near-empty hashes were paired");

        // The mirror at the other end: one bit short of every bit set.
        let out = group(
            &[(1, !(1u64 << 3)), (2, !(1u64 << 40))],
            EXACT_RECALL_DISTANCE,
        );
        assert!(out.is_empty(), "two near-full hashes were paired");
    }

    /// The rule is "would pair with anything", not "has few bits set": two hashes whose
    /// popcounts sum past the automatic bound pair on their pictures, and a plain popcount
    /// floor would throw that real match away. Five bits each at distance 8: 10 > 8, so
    /// neither is automatic, and they are 4 bits apart.
    #[test]
    fn two_sparse_hashes_that_are_not_automatic_still_pair() {
        let a = 0b1_1111u64;
        let b = 0b111u64 | (0b11u64 << 30);
        assert_eq!((a ^ b).count_ones(), 4);
        let mut out = group(&[(1, a), (2, b)], 8);
        out.sort();
        assert_eq!(out, vec![(1, 1), (2, 1)]);
    }

    #[test]
    fn distance_zero_groups_nothing() {
        let out = group(&[(1, 5), (2, 5)], 0);
        assert!(out.is_empty(), "distance 0 means the feature is off");
    }

    /// The end-to-end property: two files that are the same picture at different sizes, both
    /// with thumbnails, end up in one group; an unrelated third does not.
    ///
    /// The fixtures are written as real JPEGs and thumbnailed through `ThumbCache`, because
    /// what the pass hashes is the cached grid thumbnail, not the photo - so a test that
    /// handed it the source images would not exercise the path that exists.
    #[test]
    fn the_pass_groups_the_same_picture_at_two_sizes() {
        let (dir, lib) = temp_library();
        let photos = dir.path().join("pics");
        let (_w, folder) = seed_folder(&lib, &photos);
        let cache = ThumbCache::new(dir.path().join("cache"));

        // The blocky pattern is the pair, not the gradient: `picture`'s hash has only two or
        // three bits set, so a gradient pair would group partly for having nothing in it,
        // and this test would still pass if `dhash` returned near-nothing for everything.
        // The pattern's hash has 46 bits set, so the pair groups on structure that survived
        // the resize. Their sizes are whole multiples of the 9x8 reduction grid, so the
        // blocks land on it at both resolutions rather than straddling it.
        let files = [
            ("big.jpg", unrelated_pattern(180, 120), 10, 100),
            ("small.jpg", unrelated_pattern(72, 48), 11, 101),
            ("other.jpg", picture(300, 200), 12, 102),
        ];
        let mut items = Vec::new();
        for (name, img, size, mtime) in &files {
            let src = write_file(&photos, name, &encode(img, ImageFormat::Jpeg));
            let path = src.to_str().unwrap().to_string();
            // The row's size and mtime are the fixture's, not the file's: they are only the
            // fingerprint's inputs here, and naming them is what lets the thumbnail be
            // written under the very key the pass will look for.
            cache
                .generate(&src, 1, fingerprint(&path, *size, *mtime))
                .unwrap();
            items.push(item_at(folder, &path, *size, *mtime));
        }
        let ids = lib.insert_items(&items).unwrap();
        for id in &ids {
            lib.set_thumb_state(*id, ThumbState::Ready, None).unwrap();
        }

        let outcome = update(&lib, &cache, EXACT_RECALL_DISTANCE, &AtomicBool::new(false)).unwrap();
        assert_eq!(outcome.hashed, 3, "every thumbnailed photo takes a hash");
        assert!(outcome.groups_changed, "the pair was not grouped");

        let alike = lib.similar_of(ids[0]).unwrap();
        assert_eq!(
            alike.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec![ids[1]],
            "the same picture at two sizes is one group"
        );
        assert!(
            lib.similar_of(ids[2]).unwrap().is_empty(),
            "an unrelated picture was grouped with them"
        );

        // A second pass has nothing left to hash and nothing to regroup: a photo takes this
        // path once, and the groups it computes are the ones already stored.
        assert_eq!(
            update(&lib, &cache, EXACT_RECALL_DISTANCE, &AtomicBool::new(false)).unwrap(),
            PassOutcome::default()
        );

        // Changing the distance is a regroup and nothing else - the case the grid refresh
        // would miss if it were gated on rows hashed. Task 6 makes this a user setting.
        let outcome = update(&lib, &cache, 0, &AtomicBool::new(false)).unwrap();
        assert_eq!(outcome.hashed, 0);
        assert!(
            outcome.groups_changed,
            "the distance changed the groups and the pass did not say so"
        );
        assert!(lib.similar_of(ids[0]).unwrap().is_empty());
    }

    /// A thumbnail that is not on disk yet leaves the row a candidate for the next pass,
    /// rather than failing the whole run or marking the photo hashed.
    #[test]
    fn a_photo_whose_thumbnail_is_missing_stays_a_candidate() {
        let (dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
        let cache = ThumbCache::new(dir.path().join("cache"));
        let ids = lib
            .insert_items(&[item_at(folder, "/pics/a.jpg", 10, 100)])
            .unwrap();
        lib.set_thumb_state(ids[0], ThumbState::Ready, None)
            .unwrap();

        assert_eq!(
            update(&lib, &cache, EXACT_RECALL_DISTANCE, &AtomicBool::new(false)).unwrap(),
            PassOutcome::default()
        );
        assert_eq!(lib.similar_candidates().unwrap().len(), 1);
    }

    /// Cancelling stops between photos and keeps what was hashed. With the flag already set
    /// the pass hashes nothing at all, and still regroups.
    #[test]
    fn a_cancelled_pass_hashes_nothing_and_still_regroups() {
        let (dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/pics"));
        let cache = ThumbCache::new(dir.path().join("cache"));
        let ids = lib
            .insert_items(&[
                item_at(folder, "/pics/a.jpg", 10, 100),
                item_at(folder, "/pics/b.jpg", 11, 101),
            ])
            .unwrap();
        lib.set_similar_groups(&[(ids[0], ids[0]), (ids[1], ids[0])])
            .unwrap();

        let outcome = update(&lib, &cache, EXACT_RECALL_DISTANCE, &AtomicBool::new(true)).unwrap();
        assert_eq!(outcome.hashed, 0);
        assert!(outcome.groups_changed, "the stale group was left in place");
        assert!(
            lib.similar_of(ids[0]).unwrap().is_empty(),
            "the regroup runs even when nothing was hashed"
        );
    }
}
