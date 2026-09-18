# photon — Duplicate Finder Design

**Date:** 2026-09-18
**Status:** Approved design, implemented
**Builds on:** v0.14.2, schema 7 → 8

## 1. Scope

Exact duplicates only: files with identical bytes. Near-duplicates (resized, re-saved, bursts)
were considered and not chosen. photon never deletes a photo, so the finder only *reports*: a
Duplicates view, and each photo's copies in the viewer. Acting on them is the file manager's job.

## 2. Reading as little as possible

Two files of different sizes cannot be identical, and a photo's exact byte size is close to
unique. So a file is hashed only if another live file has exactly its size
(`Library::hash_candidates`, served by the partial index `items_size`). On a real library that
is little more than the duplicates themselves; the rest of the library is never opened.

The hash is XXH3-128 (`xxhash-rust`, already a dependency for thumbnail fingerprints), stored
in `items.content_hash BLOB`, NULL for nearly every row. It is not cryptographic and need not
be: nobody forges collisions against their own library, and an accidental one between two
same-sized files does not happen at 128 bits.

Offline roots are skipped: they are rescanned every 30 seconds while away, and each candidate
would fail to open every time. An unreadable file is skipped and stays a candidate.

## 3. When it runs

At the end of `Engine::run_scan`, after the scan has reported done, on the scan's own thread.

- **In the engine, not the scanner.** A duplicate is a fact about the whole library, not about
  the root or subtree one scan walked; and `walk_tree` has two callers, which a scanner pass
  has to remember and this does not.
- **After every scan, changed rows or not.** The first scan after the upgrade touches nothing
  and still has the whole library to hash. With nothing to do the pass is one indexed query.
- **After "done".** Hashing many duplicates on a slow drive takes minutes, during which the
  status bar should not claim a scan is running. `cancel_scan` still waits for it, and it
  checks the scan's cancel flag between files and between 1 MiB chunks.
- **One pass at a time** (`Engine.hashing`). A scan that finds it running sets
  `hash_requested` and leaves; the runner loops while the flag is set and re-checks after
  dropping the guard, so files indexed after its candidate list was read are not left for the
  next launch, and no request is lost in the hand-over.
- A pass that stored any hash rebuilds the grid, since the view and its count derive from them.

## 4. Staying true

- `update_items` sets `content_hash = NULL`: the hash described bytes the file no longer holds,
  and a row with a hash is never a candidate again, so nothing else would correct it.
- `set_content_hash` writes only if the row still has the size and mtime the candidate was
  listed with. The pass runs well after the scan; a file rewritten in between must not get a
  hash of its new bytes under its old fingerprint.
- Every query counts live rows only. A photo whose one twin went missing is not a duplicate.

## 5. The view

`GridView::Duplicates` is a *filter*, like Starred: photos whose hash another live photo
shares, in the normal folder-first order, with the filter applied to the driver as well as the
outer `WHERE`. A group-ordered view (each set of copies adjacent) was rejected: any order that
is not folder-first breaks sections, the sidebar and the viewer caption at once, as Recent did.
Folder-first also answers the usual question — *which folders* are copies of each other —
since the sidebar lists exactly the folders holding duplicates, with counts. Pairing is shown
per photo: `ViewerItem.copies` lists the other paths, and a click locates one.

`GridInfo.duplicateCount` counts photos (it labels a view of photos). The sidebar row appears
only while the count is above zero or the view is active.

## 6. Testing

Core tests use real files: same-size-but-different bytes must not match (the case the size
shortcut alone gets wrong), a rewritten file leaves the view, a stale candidate is refused, a
missing twin ends the pair, an unreadable file stays a candidate, a cancelled pass stores
nothing. Two plan tests pin the indexes. An engine test pins the wiring end to end. Each was
probed by reverting its change; one probe passed, exposing an `IS NOT NULL` "planner hint"
that did nothing, and it was removed along with the comment that justified it. The sidebar row
and the copies list are component wiring: two README checklist lines.
