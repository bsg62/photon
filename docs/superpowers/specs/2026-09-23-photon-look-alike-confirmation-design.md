# Confirming look-alikes by their pixels

2026-09-23

## The problem

A user reports false look-alikes on Conservative: two different photos of the same person in
nearly the same pose, grouped as "the same picture". The photos are private and on another
machine, so nothing below was measured on them.

The cause is the hash, not the threshold. The 64-bit dHash reduces a photo to 9×8 greyscale.
At that size a person standing in the same place in a similar posture *is* the same picture,
and the two hashes land 0–5 bits apart - as close as a genuine copy. No threshold separates
them, and lowering Conservative would lose real copies while keeping these.

## The measurement

A throwaway probe (not committed) built grid thumbnails through `ThumbCache::generate` and
hashed them with `similar::dhash`, exactly the production path, over 17 public-domain NASA
portraits from Wikimedia Commons. Among them are three real same-session pairs of one person
in a similar pose. From each portrait it made five genuine copies (50%, 25% and 12.5% size;
re-compressed at q50; a 1280px q70 "chat app" copy), one tone-shifted re-export, and three
"second shots" with the frame moved 1%, 2% and 4%.

| pair | dHash-64 bits | dHash-256 bits | pixel difference\* |
|---|---|---|---|
| genuine copy (255 pairs) | 0-6 | 0-37 | 0.2-2.3 |
| tone-shifted copy | 0-3 | 1-25 | 0.2-0.6 |
| frame moved 1% / 2% / 4% | 0-7 | 3-50 | from 1.4 / ~2.4 / ~4.4 |
| real same-pose pair | 6-19 | 65-115 | 19-47 |
| unrelated | 7-52 | 41-160 | from 7.5 |

\* mean absolute difference, 0-255, of the two grid thumbnails reduced to 32×32 greyscale,
each with its own mean brightness subtracted first.

A finer hash (16×16 dHash) is *worse*: flat studio backgrounds turn compression noise into
flipped bits, so genuine copies spread up to 37 bits apart. The pixel difference is the one
measure with a wide gap between copies (≤ 2.3) and different shots (≥ 19 for the real pairs).

## The design

**The hash finds candidates; the pixels confirm them.** `similar::group` keeps its banding and
its distance test, and a pair within the distance is united only if a `same_picture` check
also agrees. In `update` that check reduces both photos' cached grid thumbnails to 32×32
greyscale and compares them with the mean removed; a pair is the same picture when the
difference is at most `SAME_PICTURE_MAX_DIFFERENCE = 4.0`. That sits above every genuine copy
measured (2.3) with room for a noisier encoder, below the real different shots (19) by a wide
margin, and above a 1% frame shift - two shots that differ by two pixels at thumbnail size
are, to a person, the same picture.

- **A pair already in one group is not re-confirmed.** Union-find is checked first, so a
  confirmation is spent only on a pair that would join two groups. Chaining still happens,
  but every link in the chain is a confirmed one.
- **An unreadable thumbnail confirms nothing.** The action taken from Duplicates is deleting
  a file, so a missed pair is cheaper than a false one - the same reasoning that made
  Conservative the default. The next pass tries again.
- **The reductions are cached in memory, in the engine, keyed by thumbnail key.** The regroup
  runs after every scan, including the watcher's; without a cache every grouped photo's
  thumbnail is decoded again each time. The key already changes when the file or its edit
  does, so the cache needs no invalidation of its own; entries not used by a pass are
  dropped at its end, so it is bounded by the photos that have a candidate pair. The cache
  lives in the `hashing` mutex, which already serialises every pass.
- **No schema change, no setting change.** Conservative stays 3 and Loose stays 6. The probe
  showed genuine copies up to 6 bits apart, so Conservative misses some heavily shrunk
  copies; with confirmation in place Loose no longer trades that recall for false pairs,
  and its help text says so. Moving the default is a separate decision.
- Existing groups are corrected by the first pass after the upgrade: the regroup is
  whole-library and unconditional, and `set_similar_groups` writes only on a difference.

## Tests, and what each pins

- `a_close_pair_that_is_not_the_same_picture_is_not_grouped` - `group` with a check that
  refuses: nothing is grouped. Fails if the check is ignored.
- `a_pair_already_joined_is_not_confirmed_again` - counts calls to the check across a chain.
  Fails if union-find is not consulted first.
- `a_copy_is_the_same_picture_and_a_moved_frame_is_not` - `picture_difference` on a textured
  fixture: resized and re-encoded ≤ the limit, moved by a few percent > the limit. Fails if
  the limit or the mean removal is wrong way round.
- `a_tone_change_is_still_the_same_picture` - fails without the mean removal.
- `the_pass_does_not_group_two_shots_that_hash_alike` - end to end through `ThumbCache` and
  `update`: two JPEGs whose dHashes are within 3 but whose pictures differ at 32×32 stay
  ungrouped. Fails if `update` passes an always-true check.
- `the_pass_groups_the_same_picture_at_two_sizes` (existing) keeps pinning recall through
  the new check.
- `the_pass_reuses_cached_reductions` - a second pass with the thumbnails deleted from disk
  still groups the pair. Fails if the cache is not consulted.
