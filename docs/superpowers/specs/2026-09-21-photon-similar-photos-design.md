# Similar photos

2026-09-21

photon finds byte-identical files today. The README says so plainly, and then says what it
misses: "resized or re-saved versions are different files and are not reported." That is the
case people actually have — the same photo at 2048px from an email, a re-exported JPEG from
Lightroom, a copy that went through a chat app and came back. This adds those to the existing
Duplicates view.

## Decisions taken before the design

- **Look-alikes extend the Duplicates view**; there is no second sidebar row. One place to
  look, exact copies and look-alikes together, each marked for what it is.
- **Conservative by default**, with the distance exposed in Settings. The action a person
  takes from this view is deleting a file in their file manager, so a false pair costs more
  than a missed one.

## What a person sees

The **Duplicates** sidebar row keeps its meaning — "photos that have another copy" — and
widens to include photos that have a *look-alike*. In the viewer's info panel, the existing
list of copies gains a second group: **Identical** (what it lists today) and **Looks the
same**, each entry still clicking through to locate that photo. A look-alike entry carries
its pixel dimensions, because at that point the question is almost always "which of these is
the big one".

Settings gains **Duplicates → Find look-alikes**: *Off* / *Conservative* (default) /
*Loose*. Off restores exactly today's behaviour. The three choices are stored as the Hamming
distance they mean — `0`, `3`, `6` — rather than as names, because the distance is what the
pass actually uses and a stored name would need a second table to interpret it. A stored
value outside those three is clamped into range rather than refused, the way an unknown theme
falls back.

## The hash

A 64-bit difference hash (dHash) over the photo as shown: reduce to 9×8 greyscale, and set
one bit per horizontal neighbour pair according to which is brighter. It is the cheapest hash
with the property that matters here — invariant to scale and to re-compression, because it
encodes the relationship between regions rather than their values.

**It is computed in the thumbnail renderer, not in `describe()` and not in a pass of its
own.** The renderer has already decoded and downscaled the photo; the hash is a few
microseconds on top of a ~175 ms render. Any other placement pays for a second decode of
every file in the library, which is the single most expensive thing photon does.

Two consequences follow from that placement and both must be stated rather than discovered:

- **A photo has no perceptual hash until its thumbnail has been rendered.** That is not a
  gap in practice — `ThumbService::enqueue_pending` sweeps every pending row, so a library
  converges — but it does mean the look-alikes in a freshly indexed folder appear as its
  thumbnails land, not the instant the scan ends.
- **The hash is of the photo as photon shows it**, which for an edited photo means after
  EXIF orientation, turns and crop. A photo and its own cropped version are therefore *not*
  look-alikes of each other, which is the right answer: they are one photo with one edit.

`items.percep_hash` is an `INTEGER` (the 64 bits, NULL until rendered). Like `content_hash`,
**`update_items` must set it back to NULL** when a file's fingerprint moves, or a re-saved
photo keeps its old neighbours forever.

## Grouping

Pairwise comparison is 5×10⁹ popcounts on a 100k library — once per scan, but seconds of it,
and it scales quadratically. Instead the pass reads every `(id, hash)` into memory (100k rows
is 1.6 MB), splits each hash into **four 16-bit bands**, and buckets by band. Two hashes
within Hamming distance 3 must agree exactly on at least one band — four bands, at most three
differing bits, so some band has none — which makes the band buckets an *exact* candidate
filter at the conservative setting, not a heuristic. Only candidates sharing a bucket are
compared properly.

- **Conservative** is distance ≤ 3. Recall is complete: the pigeonhole argument above.
- **Loose** is distance ≤ 6. The same buckets are used, so recall is **best-effort** — a pair
  at distance 4–6 is found only if it happens to share a band. This is a deliberate trade
  (the alternative is six bands and a much larger candidate set) and the Settings help text
  says "finds most" rather than "finds all".

No SQL index is involved and none should be added: the banding is in Rust, over a list that
is small because it is one integer per photo.

Pairs are resolved into groups by union-find, and each member row takes the group's smallest
member id in `items.similar_group` (NULL when a photo has no look-alike). The Duplicates
filter becomes "has an identical twin **or** `similar_group IS NOT NULL`", which keeps the
view a plain filter — so it keeps the folder-first order, the sidebar agreement and
everything else built on `grid_query`.

## Where the pass runs

`Engine::hash_duplicates` already runs at the end of every `run_scan`, in the engine rather
than the scanner, so that neither of `walk_tree`'s two callers can be forgotten and because a
duplicate is a fact about the whole library. `group_similar` runs in exactly the same place,
immediately after it, for exactly the same reasons.

It is a whole-library recompute, not an incremental update. A union-find over 100k rows is
milliseconds, and the incremental version has to reason about a group *splitting* when a
photo is purged — the kind of state that goes subtly wrong and is never noticed.

`ScanReport` needs nothing new: this pass runs outside the scan, alongside
`hash_duplicates`, and follows whatever refresh that already triggers.

## Schema 12

```sql
ALTER TABLE items ADD COLUMN percep_hash INTEGER;
ALTER TABLE items ADD COLUMN similar_group INTEGER;
CREATE INDEX items_similar_group ON items(similar_group) WHERE similar_group IS NOT NULL;
```

The partial index serves the Duplicates filter and gets a plan test, as
`the_recent_view_is_served_by_its_index` does for Recent. Both `library/mod.rs` version
assertions — the opened version and `SchemaTooNew`'s `supported` — move to 12 by hand; that
hardcoding is the tripwire. The table count assertion does **not** move: this migration only
alters `items` and adds an index, so a changed table count would mean something unintended
was created.

No `bump_thumb_gc_epoch`: neither column is part of the thumbnail key, so neither write can
orphan a thumbnail.

## IPC

`ItemCopy` gains `kind: 'identical' | 'similar'` and `width`/`height`. `copies_of` returns
both groups, identical first. New settings pair `similarDistance` / `setSimilarDistance`
following `slideshowInterval` exactly — command, wrapper, handler, `generate_handler!`, the
TS mirror in `api.ts`, and an answer in `screenshots/mock.js` (the test in `screenshots.rs`
fails otherwise).

## Tests, and which of them discriminate

The rule here is that a new test must be shown to fail with its change reverted, so each of
these names the revert it is pinning:

- `dhash_survives_a_resize` — the same photo at full size and at 25% hash within distance 3.
  Fails if the hash is computed over raw bytes rather than the reduced greyscale.
- `dhash_separates_different_photos` — two unrelated fixtures exceed distance 6. This is the
  one that would catch a hash that is accidentally constant.
- `bands_find_every_pair_within_three` — brute-force every pair in a synthetic set and assert
  the banded candidate filter found all of them. Fails if the band count or width is changed,
  which is what makes the pigeonhole argument a property of the code and not of this document.
- `groups_are_transitive` — A~B, B~C, A≁C all land in one group. Fails if union-find is
  replaced by pairwise storage.
- `a_replaced_file_loses_its_similar_group` — rewrite a file, rescan, assert both
  `percep_hash` and `similar_group` are NULL. Fails if `update_items` forgets the new columns,
  which is the defect most likely to ship.
- `off_restores_todays_duplicates_view` — with the setting Off, the view's rows equal the
  exact-duplicate rows. Fails if the filter unconditionally includes `similar_group`.

The info panel's two-group rendering has no test — there is no component harness — and goes
on the README's smoke checklist instead.

## Not in this design

Grouping a photo with its own crop or colour edit (the edit *is* the relationship, and photon
already knows it). Choosing a keeper or deleting anything: photon never deletes a photo, and
this changes nothing about that. Any similarity beyond "the same picture" — no clustering by
scene, no "photos like this one".
