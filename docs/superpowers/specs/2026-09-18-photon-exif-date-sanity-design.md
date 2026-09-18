# photon — EXIF Date Sanity Design

**Date:** 2026-09-18
**Status:** Approved design, implemented
**Reverses:** the 2026-09-14 decision to trust any EXIF date
**Builds on:** v0.14.2

## 1. The problem

A folder is filed in the sidebar and placed in the grid by its photos' capture dates. photon
believed any EXIF date whose year, month and day were non-zero, so one file dated 4501 put
its whole folder at the top of the library, under a year nobody can scroll past. The user hit
this on a real library on 2026-09-14 with two such files.

## 2. The rule

A capture date is believed only if it lies **between 1970-01-01 and one day after the moment
of the scan**. Anything else is treated as absent, and the photo is dated by its file mtime —
the path a photo with no EXIF already takes.

- The lower bound catches cameras whose clock was never set (0000, 1900). Its cost, accepted:
  a scan deliberately back-dated in EXIF to before 1970 falls back to its mtime.
- The day of slack exists because `taken_at` is the camera's naive local time read as UTC, so
  an honest photo taken now in UTC+14 reads fourteen hours into the future.
- The three date tags are tried in the existing order (`DateTimeOriginal`, `DateTimeDigitized`,
  `DateTime`) and the check applies to each: a sane later tag beats an implausible earlier one.

## 3. Existing libraries

`EXIF_VERSION` goes from 1 to 2, so every unchanged file is re-described once on its folder's
next scan. `update_item_meta` now writes `taken_at` as well; until this change it deliberately
did not, on the reasoning that the date had been read correctly the first time, which is no
longer true. For a photo whose date was sane, the value written equals the value stored, so
nothing moves. `enriched` already feeds `touched_rows`, so the grid rebuilds.

No schema change. No UI change. Thumbnails are keyed by path, size and mtime and are untouched.

## 4. Testing

- `plausible_taken_at` is pure and takes the clock as an argument; its bounds are tested
  directly. `read_image_meta_at(path, now)` is the seam for the reader.
- A scanner test mis-dates a row the way an old reader would have and asserts the next scan
  re-dates it. It fails with the filter removed, with `taken_at` dropped from the backfill's
  `UPDATE`, and with the version bump reverted — each probed.
