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
/// Candidate pairs are found by splitting each hash into four 16-bit bands, bucketing by
/// band, and comparing each bucket with itself and with the sixteen buckets one bit away in
/// the same band. Two hashes within Hamming distance 7 must be within one bit of each other
/// on at least one band - four bands, at most seven differing bits, so some band has at most
/// one. That makes the probe an exact filter at this distance, not a heuristic. It was 3
/// with exact buckets alone (some band then has *no* differing bit), and genuine copies were
/// measured up to 6 bits apart - a sixteenth-size re-encode - so the conservative setting
/// missed them; the pixel check behind the hash (`same_picture`) is what makes a wider net
/// safe to cast.
///
/// Exact about *recall*, not about cost: `pairs_with_anything` keeps the pathological case
/// (every blank frame sharing one hash) out of the buckets, but a bucket is a shared 16-bit
/// band and not an excluded one, so a library with many near-identical-but-not-empty
/// pictures can still put a large cluster in one bucket and pay its quadratic comparison.
/// Usually bounded, not always. Above it, a pair is found only if one of its bands happens
/// to be that close, which is why the UI says "finds most" rather than "finds all".
pub const EXACT_RECALL_DISTANCE: u32 = 7;

const BANDS: u32 = 4;
const BAND_BITS: u32 = 16;

/// The edge of the square two thumbnails are reduced to before their pixels are compared.
/// Fine enough that a moved arm or a turned head shows, coarse enough that JPEG noise and a
/// resize's resampling have averaged away.
const COMPARE_EDGE: u32 = 32;

/// The largest [`picture_difference`] at which two thumbnails are still the same picture.
///
/// Measured, not chosen (spec `2026-09-23-photon-look-alike-confirmation-design.md`): over
/// real photographs, every genuine copy - down to 12.5% size, re-compressed at q50, through
/// a chat app - came in at 2.3 or less, while real second shots of one person in a similar
/// pose were 19 and more, and unrelated photos 7.5 and more. 4 leaves an encoder noisier
/// than the ones measured some room without coming near the different shots. A frame moved
/// by 1% (two pixels at thumbnail size) still passes: at that point it is the same picture
/// to a person too.
pub const SAME_PICTURE_MAX_DIFFERENCE: f64 = 4.0;

/// A thumbnail reduced for comparison: `COMPARE_EDGE` squared greyscale values and their
/// mean. Bytes rather than mean-centred floats, because [`Reductions`] keeps one for every
/// photo with a candidate pair for the whole session: 1 KB each instead of 4.
#[derive(Clone, Debug, PartialEq)]
pub struct Reduction {
    grey: Box<[u8]>,
    mean: f32,
}

/// Reduces `img` for [`picture_difference`].
///
/// The mean comes off because a re-export with its exposure nudged is still the same
/// photo, and a uniform shift in brightness would otherwise count at every one of the
/// 1024 pixels. What is left is the arrangement, which is what differs between two shots.
pub fn reduce(img: &DynamicImage) -> Reduction {
    let grey = img
        .resize_exact(COMPARE_EDGE, COMPARE_EDGE, FilterType::Triangle)
        .to_luma8();
    let grey = grey.into_raw().into_boxed_slice();
    let mean = grey.iter().map(|&v| f32::from(v)).sum::<f32>() / grey.len() as f32;
    Reduction { grey, mean }
}

/// How far apart two reduced pictures are: the mean absolute difference per pixel, on the
/// 0-255 greyscale.
pub fn picture_difference(a: &Reduction, b: &Reduction) -> f64 {
    let sum: f64 = a
        .grey
        .iter()
        .zip(b.grey.iter())
        .map(|(&x, &y)| f64::from(((f32::from(x) - a.mean) - (f32::from(y) - b.mean)).abs()))
        .sum();
    sum / a.grey.len() as f64
}

/// Whether two reduced pictures are the same picture; see [`SAME_PICTURE_MAX_DIFFERENCE`].
pub fn same_picture(a: &Reduction, b: &Reduction) -> bool {
    picture_difference(a, b) <= SAME_PICTURE_MAX_DIFFERENCE
}

/// Reductions kept from one pass to the next, by thumbnail key.
///
/// The regroup runs after every scan - the watcher's too, seconds after a single file
/// lands - and confirms every nominated pair again, so without this each pass would decode
/// the thumbnail of every photo that has a look-alike. The key changes whenever the file or
/// its edit does, so an entry can never describe a picture its row no longer shows, and
/// needs no invalidation of its own. A pass keeps only the entries it used, which bounds
/// the cache by the photos that have a candidate pair rather than letting old keys pile up.
#[derive(Debug, Default)]
pub struct Reductions {
    by_key: HashMap<u64, Reduction>,
}

/// Groups look-alikes, returning `(item_id, group_id)` for every photo that has at least one.
///
/// `group_id` is the smallest item id in the group, so a group has a stable name that does
/// not depend on iteration order. A photo that resembles nothing is absent from the result
/// rather than present with a NULL - the caller clears the column wholesale first.
///
/// `distance` of 0 turns the feature off and returns nothing.
///
/// A hash within `distance` only *nominates* a pair; `same_picture` (called with the two
/// indexes into `hashes`) decides it. The 9x8 reduction behind the hash cannot tell a copy
/// from a second shot of the same pose, and the check can - see [`picture_difference`]. It
/// is asked only about a pair that would join two groups, never about one already joined
/// through a third photo: that saves the check's cost and cannot change the result, since
/// the pair is connected either way. Chains still form, but only out of confirmed links.
///
/// Whole-library, not incremental: a union-find over 100k rows is milliseconds, and the
/// incremental version has to reason about a group *splitting* when a photo is purged.
pub fn group(
    hashes: &[(i64, u64)],
    distance: u32,
    mut same_picture: impl FnMut(usize, usize) -> bool,
) -> Vec<(i64, i64)> {
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
    let mut consider = |a: usize, b: usize| {
        if a != b
            && (hashes[a].1 ^ hashes[b].1).count_ones() <= distance
            && find(&mut parent, a) != find(&mut parent, b)
            && same_picture(a, b)
        {
            union(&mut parent, a, b);
        }
    };
    for (&(band, key), indexes) in &buckets {
        for (i, &a) in indexes.iter().enumerate() {
            for &b in &indexes[i + 1..] {
                consider(a, b);
            }
        }
        // The buckets one bit away, each neighbouring pair of buckets visited once: from
        // the lower key.
        for bit in 0..BAND_BITS {
            let other = key ^ (1 << bit);
            if other < key {
                continue;
            }
            if let Some(neighbours) = buckets.get(&(band, other)) {
                for &a in indexes {
                    for &b in neighbours {
                        consider(a, b);
                    }
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
    /// How many rows took a new hash. `percep_hash` sits in no view filter and no
    /// `GridInfo` field - only the materialised `similar_group` the regroup writes decides
    /// Duplicates membership - so hashing a row can never by itself change what a grid
    /// shows, and `Engine::hash_after_scan` does not gate its refresh on this. It stays a
    /// field of its own, not folded away, because it is what the tests here assert on to
    /// pin *which* rows a pass actually hashed (candidates vs. `Off` vs. cancellation),
    /// separately from whether the regroup moved anything.
    pub hashed: u64,
    /// Whether the regroup changed which photos are in which group. Separate from `hashed`
    /// because the two come apart: changing the distance setting regroups the whole library
    /// having hashed nothing, and a photo purged out of a group does the same. This is the
    /// one signal that means the Duplicates view moved, and the only one
    /// `Engine::hash_after_scan` refreshes the grid on.
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
    reductions: &mut Reductions,
) -> Result<PassOutcome> {
    let mut hashed = 0;
    // Off means off. The grouping below returns nothing at distance 0, so hashing first
    // would be work no one can see - and it is the expensive half: on the first pass over
    // an existing library every photo is a candidate, so a user who has turned the feature
    // off would still pay a whole-library thumbnail decode at the end of every scan. The
    // regroup below still runs, because turning it off has to *clear* the groups already
    // stored.
    let candidates = if distance == 0 {
        Vec::new()
    } else {
        lib.similar_candidates()?
    };
    for candidate in candidates {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let path = cache.path_for(candidate.thumb_key, ThumbSize::Grid);
        match image::open(&path) {
            // False when the row moved on while the thumbnail was being read - the file
            // rewritten, or the user editing the photo, both of which make this the hash of
            // a picture the row no longer shows. Not a failure: the write is refused, the
            // row keeps its cleared `percep_hash`, and the next pass hashes the new picture.
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
    let photos = lib.percep_hashes()?;
    let hashes: Vec<(i64, u64)> = photos.iter().map(|p| (p.id, p.hash)).collect();
    // `None` is a thumbnail that could not be read, remembered for this pass only so it is
    // not retried for every pair it is in. It confirms nothing: a missed pair costs less than
    // a false one, and the next pass tries again.
    let mut seen: HashMap<u64, Option<Reduction>> = HashMap::new();
    // Confirming decodes thumbnails, and on the first pass after a launch the cache is empty,
    // so this half is no longer the pure arithmetic it was: it has to honour `cancel`, or a
    // quit waits it out. A cancelled regroup writes nothing - answering "no" to the remaining
    // pairs instead would dissolve real groups - and the next pass does the work.
    let mut cancelled = false;
    let groups = group(&hashes, distance, |a, b| {
        if cancelled || cancel.load(Ordering::Relaxed) {
            cancelled = true;
            return false;
        }
        let keys = [photos[a].thumb_key, photos[b].thumb_key];
        for key in keys {
            seen.entry(key).or_insert_with(|| {
                reductions
                    .by_key
                    .remove(&key)
                    .or_else(|| read_reduction(cache, key))
            });
        }
        match (&seen[&keys[0]], &seen[&keys[1]]) {
            (Some(a), Some(b)) => same_picture(a, b),
            _ => false,
        }
    });
    reductions.by_key = seen
        .into_iter()
        .filter_map(|(key, reduction)| Some((key, reduction?)))
        .collect();
    let groups_changed = !cancelled && lib.set_similar_groups(&groups)?;
    Ok(PassOutcome {
        hashed,
        groups_changed,
    })
}

fn read_reduction(cache: &ThumbCache, key: u64) -> Option<Reduction> {
    match image::open(cache.path_for(key, ThumbSize::Grid)) {
        Ok(img) => Some(reduce(&img)),
        Err(err) => {
            tracing::debug!(key, %err, "could not read a thumbnail to confirm a look-alike");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::Edit;
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

    /// Groups on the hash alone, confirming every pair it nominates: the tests of banding,
    /// transitivity and emptiness are about the hash, and a picture check would only hide
    /// what they measure.
    fn by_hash(hashes: &[(i64, u64)], distance: u32) -> Vec<(i64, i64)> {
        group(hashes, distance, |_, _| true)
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
        let out = by_hash(&[(1, 0xffff_ffff_0000_0000), (2, 0x0000_0000_ffff_ffff)], 3);
        assert!(out.is_empty());
    }

    #[test]
    fn a_close_pair_shares_the_smaller_id_as_its_group() {
        // One bit apart.
        let out = by_hash(&[(7, 0b1010), (4, 0b1011)], 3);
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
        let mut out = by_hash(&[(1, a), (2, b), (3, c)], 3);
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
            // Each of the low four bits of `i` flips one bit in its own band, at a position
            // that varies with `i`; the fifth adds a second flip in band 0. Two hashes then
            // differ by 0, 1 or 2 bits per band, so the set holds pairs that differ in
            // *every* band while staying within the exact-recall distance - the pairs only
            // the one-bit probe finds - as well as pairs beyond it.
            let mut h = base;
            for band in 0..4i64 {
                if (i >> band) & 1 == 1 {
                    h ^= 1u64 << (16 * band + (i * (band + 3) + band) % 16);
                }
            }
            if (i >> 4) & 1 == 1 {
                h ^= 1u64 << ((i * 11 + 7) % 16);
            }
            hashes.push((i + 1, h));
        }
        // Each pair grouped on its own. Grouped all at once, union-find would join a missed
        // pair through a third hash near both - the base is near everything here - and the
        // test would pass with the probe gone; it did, until it was written this way.
        let mut every_band = 0;
        for (ia, ha) in &hashes {
            for (ib, hb) in &hashes {
                if ia >= ib || (ha ^ hb).count_ones() > EXACT_RECALL_DISTANCE {
                    continue;
                }
                if (0..BANDS).all(|band| (ha ^ hb) >> (band * BAND_BITS) & 0xffff != 0) {
                    every_band += 1;
                }
                assert_eq!(
                    by_hash(&[(*ia, *ha), (*ib, *hb)], EXACT_RECALL_DISTANCE).len(),
                    2,
                    "pair ({ia}, {ib}) at distance {} was missed",
                    (ha ^ hb).count_ones()
                );
            }
        }
        assert!(
            every_band > 0,
            "the fixture no longer has a pair that differs in every band"
        );
    }

    /// The pair the probe exists for: six bits apart, spread so that every band differs -
    /// two, two, one and one - so no bucket holds both and only the one-bit neighbours find
    /// it. A sixteenth-size re-encode was measured at six bits from its original.
    #[test]
    fn a_pair_that_differs_in_every_band_is_still_found() {
        let a = 0x0123_4567_89ab_cdefu64;
        let b = a ^ 0b11 ^ (0b11 << 16) ^ (1 << 32) ^ (1 << 48);
        assert_eq!(distance(a, b), 6);
        for band in 0..BANDS {
            let shift = band * BAND_BITS;
            assert_ne!((a >> shift) & 0xffff, (b >> shift) & 0xffff);
        }
        let mut out = by_hash(&[(1, a), (2, b)], EXACT_RECALL_DISTANCE);
        out.sort();
        assert_eq!(out, vec![(1, 1), (2, 1)]);
    }

    /// Every flat picture hashes to the same value, so they are all distance 0 from each
    /// other: without the skip, one group swallows every lens-cap shot, blank scan and
    /// near-black frame in the library, and the user is asked which of twenty unrelated
    /// dark frames to delete.
    #[test]
    fn a_picture_with_no_structure_is_a_look_alike_of_nothing() {
        let mut out = by_hash(
            &[
                // The blocky pattern, not the gradient: the gradient's hash has three bits
                // set, and at the exact-recall distance of 7 that is itself too empty to
                // mean anything - the rule this test is about would drop it too.
                (1, dhash(&unrelated_pattern(180, 120))),
                (2, dhash(&unrelated_pattern(72, 48))),
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
        let out = by_hash(&[(1, 1 << 3), (2, 1 << 40)], EXACT_RECALL_DISTANCE);
        assert!(out.is_empty(), "two near-empty hashes were paired");

        // The mirror at the other end: one bit short of every bit set.
        let out = by_hash(
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
        let mut out = by_hash(&[(1, a), (2, b)], 8);
        out.sort();
        assert_eq!(out, vec![(1, 1), (2, 1)]);
    }

    #[test]
    fn distance_zero_groups_nothing() {
        let out = by_hash(&[(1, 5), (2, 5)], 0);
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

        let outcome = update(
            &lib,
            &cache,
            EXACT_RECALL_DISTANCE,
            &AtomicBool::new(false),
            &mut Reductions::default(),
        )
        .unwrap();
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
            update(
                &lib,
                &cache,
                EXACT_RECALL_DISTANCE,
                &AtomicBool::new(false),
                &mut Reductions::default()
            )
            .unwrap(),
            PassOutcome::default()
        );

        // Changing the distance is a regroup and nothing else - the case the grid refresh
        // would miss if it were gated on rows hashed. Task 6 makes this a user setting.
        let outcome = update(
            &lib,
            &cache,
            0,
            &AtomicBool::new(false),
            &mut Reductions::default(),
        )
        .unwrap();
        assert_eq!(outcome.hashed, 0);
        assert!(
            outcome.groups_changed,
            "the distance changed the groups and the pass did not say so"
        );
        assert!(lib.similar_of(ids[0]).unwrap().is_empty());
    }

    /// A photograph-like surface: 24x18 cells of unrelated brightness, sampled with the frame
    /// moved right by `shift` (a fraction of the width). Fine enough that a 32x32 reduction
    /// sees every cell, so moving the frame by a few percent is a different picture there -
    /// the second shot the 9x8 hash cannot see.
    fn textured(w: u32, h: u32, shift: f32) -> DynamicImage {
        let mut img = RgbImage::new(w, h);
        for (x, y, px) in img.enumerate_pixels_mut() {
            let cx = ((x as f32 / w as f32 + shift) * 24.0) as u32;
            let cy = y * 18 / h;
            let v = ((cx * 97 + cy * 57 + cx * cy * 31 + 13) % 200 + 28) as u8;
            *px = Rgb([v, v, v]);
        }
        DynamicImage::ImageRgb8(img)
    }

    fn jpeg_round_trip(img: &DynamicImage) -> DynamicImage {
        image::load_from_memory(&encode(img, ImageFormat::Jpeg)).unwrap()
    }

    #[test]
    fn a_copy_is_the_same_picture_and_a_moved_frame_is_not() {
        let original = reduce(&textured(1200, 900, 0.0));
        let copy = reduce(&jpeg_round_trip(&textured(1200, 900, 0.0).resize(
            300,
            225,
            FilterType::Triangle,
        )));
        let moved = reduce(&textured(1200, 900, 0.03));
        assert!(
            same_picture(&original, &copy),
            "a resized, re-encoded copy was {} apart",
            picture_difference(&original, &copy)
        );
        assert!(
            !same_picture(&original, &moved),
            "a frame moved by 3% was only {} apart",
            picture_difference(&original, &moved)
        );
    }

    /// The limit held between its two nearest neighbours: the heaviest copy measured
    /// (a sixteenth of the size, re-encoded, about 2.3) and a frame moved by half a percent
    /// (about 5.7). The copy/moved-frame test above only pins the limit's order of
    /// magnitude; this is what fails if it drifts towards the unrelated photos at 7.5.
    #[test]
    fn the_limit_sits_between_the_heaviest_copy_and_a_barely_moved_frame() {
        let original = textured(1200, 900, 0.0);
        let r = reduce(&original);
        let tiny = reduce(&jpeg_round_trip(&original.resize(
            75,
            56,
            FilterType::Triangle,
        )));
        let nudged = reduce(&textured(1200, 900, 0.005));
        assert!(
            same_picture(&r, &tiny),
            "a copy at a sixteenth of the size was {} apart",
            picture_difference(&r, &tiny)
        );
        assert!(
            !same_picture(&r, &nudged),
            "a frame moved by half a percent was only {} apart",
            picture_difference(&r, &nudged)
        );
    }

    /// A re-export with its exposure nudged is the same photo. Without the mean removed,
    /// the uniform shift counts at every pixel.
    #[test]
    fn a_tone_change_is_still_the_same_picture() {
        let original = textured(400, 300, 0.0);
        let brighter = original.brighten(20);
        assert!(
            same_picture(&reduce(&original), &reduce(&brighter)),
            "a brightened copy was {} apart",
            picture_difference(&reduce(&original), &reduce(&brighter))
        );
    }

    #[test]
    fn a_close_pair_that_is_not_the_same_picture_is_not_grouped() {
        let a = 0xffff_0000u64;
        let pair = [(1, a), (2, a ^ 1)];
        assert_eq!(by_hash(&pair, 3).len(), 2, "the hash nominates the pair");
        assert!(
            group(&pair, 3, |_, _| false).is_empty(),
            "a pair the picture check refused was grouped"
        );
    }

    /// Three hashes within a bit of each other share three of their four bands, so the
    /// buckets nominate every pair up to three times. Two confirmations join all three; any
    /// further call is a thumbnail comparison that cannot change the answer.
    #[test]
    fn a_pair_already_joined_is_not_confirmed_again() {
        let a = 0xffff_0000u64;
        let mut calls = 0;
        let mut out = group(&[(1, a), (2, a ^ 1), (3, a ^ 2)], 3, |_, _| {
            calls += 1;
            true
        });
        out.sort();
        assert_eq!(out, vec![(1, 1), (2, 1), (3, 1)]);
        assert_eq!(calls, 2, "a pair already in one group was confirmed again");
    }

    /// `unrelated_pattern` with a fine checkerboard laid over it, in one of two phases. The
    /// checker's cells are a sixteenth of the frame: averaged away entirely by the 9x8 hash,
    /// which sees only the blocks underneath, and plain at 32x32, where the two phases are
    /// each other's negative. Two different pictures the hash cannot tell apart.
    fn checkered(w: u32, h: u32, phase: u32) -> DynamicImage {
        let mut img = unrelated_pattern(w, h).to_rgb8();
        for (x, y, px) in img.enumerate_pixels_mut() {
            let on = (x * 16 / w + y * 16 / h + phase).is_multiple_of(2);
            for c in &mut px.0 {
                *c = if on {
                    c.saturating_add(40)
                } else {
                    c.saturating_sub(40)
                };
            }
        }
        DynamicImage::ImageRgb8(img)
    }

    /// Writes each image as a JPEG, thumbnails it through `ThumbCache` under the key its row
    /// will have, and inserts the rows as thumbnailed.
    fn thumbnailed(
        lib: &Library,
        cache: &ThumbCache,
        photos: &Path,
        folder: i64,
        files: &[(&str, DynamicImage)],
    ) -> Vec<i64> {
        let mut items = Vec::new();
        for (n, (name, img)) in files.iter().enumerate() {
            let src = write_file(photos, name, &encode(img, ImageFormat::Jpeg));
            let path = src.to_str().unwrap().to_string();
            let (size, mtime) = (10 + n as i64, 100 + n as i64);
            cache
                .generate(&src, 1, fingerprint(&path, size, mtime))
                .unwrap();
            items.push(item_at(folder, &path, size, mtime));
        }
        let ids = lib.insert_items(&items).unwrap();
        for id in &ids {
            lib.set_thumb_state(*id, ThumbState::Ready, None).unwrap();
        }
        ids
    }

    /// The false positive this check exists for, end to end: two photos whose hashes are
    /// within the conservative distance, and whose pictures are not the same.
    #[test]
    fn the_pass_does_not_group_two_shots_that_hash_alike() {
        let (dir, lib) = temp_library();
        let photos = dir.path().join("pics");
        let (_w, folder) = seed_folder(&lib, &photos);
        let cache = ThumbCache::new(dir.path().join("cache"));
        let ids = thumbnailed(
            &lib,
            &cache,
            &photos,
            folder,
            &[
                ("a.jpg", checkered(288, 192, 0)),
                ("b.jpg", checkered(288, 192, 1)),
            ],
        );

        let mut reductions = Reductions::default();
        let outcome = update(
            &lib,
            &cache,
            EXACT_RECALL_DISTANCE,
            &AtomicBool::new(false),
            &mut reductions,
        )
        .unwrap();
        assert_eq!(outcome.hashed, 2);
        let hashes = lib.percep_hashes().unwrap();
        assert!(
            distance(hashes[0].hash, hashes[1].hash) <= EXACT_RECALL_DISTANCE,
            "the fixture no longer hashes alike, so this test proves nothing: {} bits",
            distance(hashes[0].hash, hashes[1].hash)
        );
        assert!(
            lib.similar_of(ids[0]).unwrap().is_empty(),
            "two different pictures were grouped on their hash alone"
        );
    }

    /// A second pass confirms from what the first one reduced, not from the disk: here the
    /// thumbnails are gone before it runs, and the pair still groups.
    #[test]
    fn the_pass_reuses_cached_reductions() {
        let (dir, lib) = temp_library();
        let photos = dir.path().join("pics");
        let (_w, folder) = seed_folder(&lib, &photos);
        let cache = ThumbCache::new(dir.path().join("cache"));
        let ids = thumbnailed(
            &lib,
            &cache,
            &photos,
            folder,
            &[
                ("big.jpg", unrelated_pattern(180, 120)),
                ("small.jpg", unrelated_pattern(72, 48)),
            ],
        );
        let mut reductions = Reductions::default();
        let no = AtomicBool::new(false);
        update(&lib, &cache, EXACT_RECALL_DISTANCE, &no, &mut reductions).unwrap();
        assert_eq!(
            lib.similar_of(ids[0]).unwrap().len(),
            1,
            "the pair did not group"
        );

        std::fs::remove_dir_all(dir.path().join("cache")).unwrap();
        lib.set_similar_groups(&[]).unwrap();
        let outcome = update(&lib, &cache, EXACT_RECALL_DISTANCE, &no, &mut reductions).unwrap();
        assert!(outcome.groups_changed);
        assert_eq!(
            lib.similar_of(ids[0]).unwrap().len(),
            1,
            "the second pass went back to the disk for thumbnails it had already reduced"
        );
    }

    /// A pair whose thumbnails cannot be read is not confirmed, even when the hashes say it
    /// is the same picture: a missed pair costs less than a false one, and the next pass
    /// will try again once the thumbnails are back.
    #[test]
    fn an_unreadable_thumbnail_confirms_nothing() {
        let (dir, lib) = temp_library();
        let photos = dir.path().join("pics");
        let (_w, folder) = seed_folder(&lib, &photos);
        let cache = ThumbCache::new(dir.path().join("cache"));
        let ids = thumbnailed(
            &lib,
            &cache,
            &photos,
            folder,
            &[
                ("big.jpg", unrelated_pattern(180, 120)),
                ("small.jpg", unrelated_pattern(72, 48)),
            ],
        );
        let no = AtomicBool::new(false);
        update(
            &lib,
            &cache,
            EXACT_RECALL_DISTANCE,
            &no,
            &mut Reductions::default(),
        )
        .unwrap();
        assert_eq!(
            lib.similar_of(ids[0]).unwrap().len(),
            1,
            "the pair did not group"
        );

        std::fs::remove_dir_all(dir.path().join("cache")).unwrap();
        update(
            &lib,
            &cache,
            EXACT_RECALL_DISTANCE,
            &no,
            &mut Reductions::default(),
        )
        .unwrap();
        assert!(
            lib.similar_of(ids[0]).unwrap().is_empty(),
            "a pair was confirmed without its thumbnails"
        );
    }

    /// A pass cancelled while it still had pairs to confirm leaves the stored groups as
    /// they were, rather than writing a regroup it only half did.
    #[test]
    fn a_pass_cancelled_before_confirming_writes_no_groups() {
        let (dir, lib) = temp_library();
        let photos = dir.path().join("pics");
        let (_w, folder) = seed_folder(&lib, &photos);
        let cache = ThumbCache::new(dir.path().join("cache"));
        let ids = thumbnailed(
            &lib,
            &cache,
            &photos,
            folder,
            &[
                ("big.jpg", unrelated_pattern(180, 120)),
                ("small.jpg", unrelated_pattern(72, 48)),
            ],
        );
        let no = AtomicBool::new(false);
        update(
            &lib,
            &cache,
            EXACT_RECALL_DISTANCE,
            &no,
            &mut Reductions::default(),
        )
        .unwrap();
        lib.set_similar_groups(&[]).unwrap();

        let outcome = update(
            &lib,
            &cache,
            EXACT_RECALL_DISTANCE,
            &AtomicBool::new(true),
            &mut Reductions::default(),
        )
        .unwrap();
        assert!(!outcome.groups_changed);
        assert!(
            lib.similar_of(ids[0]).unwrap().is_empty(),
            "a cancelled pass went on confirming and wrote its groups"
        );
    }

    /// An edited photo's hash and thumbnail are both of the picture as shown, stored under
    /// the edited key; confirming against the bare fingerprint's key would read the unedited
    /// thumbnail, or nothing at all once it has been collected.
    #[test]
    fn two_edited_copies_still_group() {
        let (dir, lib) = temp_library();
        let photos = dir.path().join("pics");
        let (_w, folder) = seed_folder(&lib, &photos);
        let cache = ThumbCache::new(dir.path().join("cache"));
        let turned = Edit::new(1, None).unwrap();
        let mut items = Vec::new();
        for (n, (name, img)) in [
            ("big.jpg", unrelated_pattern(180, 120)),
            ("small.jpg", unrelated_pattern(72, 48)),
        ]
        .iter()
        .enumerate()
        {
            let src = write_file(&photos, name, &encode(img, ImageFormat::Jpeg));
            let path = src.to_str().unwrap().to_string();
            let (size, mtime) = (10 + n as i64, 100 + n as i64);
            // Only the edited thumbnail exists, as it would once the unedited one had been
            // collected.
            let (preview, grid) = cache.render(&src, 1, turned).unwrap();
            cache
                .store(
                    turned.thumb_key(fingerprint(&path, size, mtime)),
                    &preview,
                    &grid,
                )
                .unwrap();
            items.push(item_at(folder, &path, size, mtime));
        }
        let ids = lib.insert_items(&items).unwrap();
        for id in &ids {
            lib.set_item_edit(*id, turned).unwrap();
            lib.set_thumb_state(*id, ThumbState::Ready, None).unwrap();
        }

        let no = AtomicBool::new(false);
        let outcome = update(
            &lib,
            &cache,
            EXACT_RECALL_DISTANCE,
            &no,
            &mut Reductions::default(),
        )
        .unwrap();
        assert_eq!(outcome.hashed, 2, "the edited thumbnails were not hashed");
        assert_eq!(
            lib.similar_of(ids[0]).unwrap().len(),
            1,
            "two edited copies were not confirmed against their edited thumbnails"
        );
    }

    /// The cache holds what the last pass used and nothing older, so it is bounded by the
    /// photos that have a candidate pair.
    #[test]
    fn a_pass_drops_the_reductions_it_did_not_use() {
        let (dir, lib) = temp_library();
        let photos = dir.path().join("pics");
        let (_w, folder) = seed_folder(&lib, &photos);
        let cache = ThumbCache::new(dir.path().join("cache"));
        thumbnailed(
            &lib,
            &cache,
            &photos,
            folder,
            &[
                ("big.jpg", unrelated_pattern(180, 120)),
                ("small.jpg", unrelated_pattern(72, 48)),
            ],
        );
        let no = AtomicBool::new(false);
        let mut reductions = Reductions::default();
        update(&lib, &cache, EXACT_RECALL_DISTANCE, &no, &mut reductions).unwrap();
        assert_eq!(reductions.by_key.len(), 2);
        update(&lib, &cache, 0, &no, &mut reductions).unwrap();
        assert!(
            reductions.by_key.is_empty(),
            "a pass that confirmed nothing kept the old reductions"
        );
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
            update(
                &lib,
                &cache,
                EXACT_RECALL_DISTANCE,
                &AtomicBool::new(false),
                &mut Reductions::default()
            )
            .unwrap(),
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

        let outcome = update(
            &lib,
            &cache,
            EXACT_RECALL_DISTANCE,
            &AtomicBool::new(true),
            &mut Reductions::default(),
        )
        .unwrap();
        assert_eq!(outcome.hashed, 0);
        assert!(outcome.groups_changed, "the stale group was left in place");
        assert!(
            lib.similar_of(ids[0]).unwrap().is_empty(),
            "the regroup runs even when nothing was hashed"
        );
    }

    /// "Off restores exactly today's behaviour" is a promise about the *work*, not only
    /// about the groups. The candidate loop opens and decodes a thumbnail per unhashed
    /// photo, and on the first pass after an upgrade that is the whole library - paid at
    /// the end of every scan by a user who has turned the feature off. The regroup still
    /// has to run, or the groups from before Off was chosen would stay on screen.
    #[test]
    fn off_hashes_nothing_and_still_clears_the_groups() {
        let (dir, lib) = temp_library();
        let photos = dir.path().join("pics");
        let (_w, folder) = seed_folder(&lib, &photos);
        let cache = ThumbCache::new(dir.path().join("cache"));

        // A real thumbnail under the key the pass would look for, so "nothing was hashed"
        // is a decision and not just a missing file.
        let src = write_file(
            &photos,
            "a.jpg",
            &encode(&unrelated_pattern(180, 120), ImageFormat::Jpeg),
        );
        let path = src.to_str().unwrap().to_string();
        cache
            .generate(&src, 1, fingerprint(&path, 10, 100))
            .unwrap();
        let ids = lib
            .insert_items(&[
                item_at(folder, &path, 10, 100),
                item_at(folder, "/pics/b.jpg", 11, 101),
            ])
            .unwrap();
        lib.set_thumb_state(ids[0], ThumbState::Ready, None)
            .unwrap();
        lib.set_similar_groups(&[(ids[0], ids[0]), (ids[1], ids[0])])
            .unwrap();

        let outcome = update(
            &lib,
            &cache,
            0,
            &AtomicBool::new(false),
            &mut Reductions::default(),
        )
        .unwrap();

        assert_eq!(
            outcome.hashed, 0,
            "Off still decoded and hashed every thumbnail"
        );
        assert!(
            lib.percep_hashes().unwrap().is_empty(),
            "Off wrote a hash nothing will ever read"
        );
        assert!(outcome.groups_changed);
        assert!(
            lib.similar_of(ids[0]).unwrap().is_empty(),
            "Off left the groups it was asked to clear"
        );
    }
}
