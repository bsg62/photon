# grid_info sends the layout only when it changed, and caches its counts

Date: 2026-09-28. Section 7 of `docs/superpowers/plans/2026-09-28-performance-audit-open-items.md`
("`grid_info` sends every section and folder on every call").

## The problem

The UI calls `grid_info` on every grid version (`LibraryStore::loadGrid`, after each
`library-changed`). Each call returns every `Section` (one per folder run) and every
`FolderTally` (one per folder): about 1 MB of JSON at 5,000 folders, serialized in Rust,
carried over IPC and parsed in the webview. It also runs four `COUNT(*)` queries, one of which
(`video_count`) reads every live row.

A grid version comes from a paced scan rebuild (at most every 250 ms), a star, a hide, a
keyword, an edit, a video poster frame (up to once a second) and each hashing pass. The
sections and folders change only when the set of photos per folder changes - a scan, a hide, a
view or sort switch. A star, a keyword, an edit, a poster frame or a hashing pass leaves them
identical and still sends them whole. The counts change only on data writes and hashing passes.

## Decisions

- **Steady state only.** The layout is left out when it did not change, and the counts are not
  recomputed when nothing they read changed. During a scan, whose rebuilds really do move
  photos between folders, the full layout is still sent. Rejected: sending deltas against the
  version the UI holds (more state kept per version, patches applied in order, more ways to go
  wrong) and a separate `grid_layout` command (a second round trip whenever the layout does
  change, which is every scan rebuild, and a consistency check between two answers).
- **One call, as today.** The UI tells `grid_info` which layout it holds; the answer leaves the
  layout out when that one is current.

## Design

### The layout generation

- The engine's published grid (`Engine.grid`, today `(version, index, failure)`) gains a
  `layout_gen: u64` beside the version, under the same lock.
- `publish` compares the new index's `sections()` and `folders()` with the index it replaces
  (`Vec` equality; cheap next to the rebuild that produced it). Different: `layout_gen + 1`;
  equal: unchanged. The comparison covers exactly what the UI draws from: a section's folder
  id, offset, count and oldest date; a folder's id, count, oldest date, bytes and modified time.
- The empty grid `Engine::open` publishes at `NOT_BUILT` has layout generation 0. A failed first
  build's empty stand-in compares like any other publish.
- `grid_info(known_layout: Option<u64>)` reads version, index, failure and `layout_gen` in one
  read of `Engine.grid` (as `grid_and_failure` does), so the layout it returns or omits is the
  one of the version it reports. It returns
  `layout: Option<GridLayout { gen, sections, folders }>`, `None` exactly when
  `known_layout == Some(layout_gen)`.
- `GridInfo` loses `sections` and `folders` and gains `layout`.

### The counts cache

- The engine gains `counts_epoch: AtomicU64` and `counts: Mutex<Option<(u64, Counts)>>`, where
  `Counts` holds `starred`, `duplicate`, `hidden` and `video`.
- `grid_info` reads the epoch *first*. When the cache holds that epoch it returns the cached
  counts; otherwise it runs the four queries and stores them under the epoch it read before
  querying - so a write racing the queries leaves the cache already stale, and the next call
  recomputes. A failed count query is logged and read as 0, as today, and not cached.
- **What bumps the epoch:**
  - `data_snapshot`, which every `refresh_grid` goes through - every data write (star, hide,
    keyword, scan, edit, album, folder removal) - beside `data_dirty`, for the same reason;
  - `hash_after_scan`, before each of its derived rebuilds: the Duplicates count reads
    `content_hash` and `similar_group` (`duplicate_ids!`). (Added after the whole-branch
    review: the regroup branch was first left out.)
- **What does not:** a poster frame's derived rebuild (`refresh_grid_derived` from a stored
  frame), and the view setters' rebuilds (`rebuild_or_restore`): the counts are library-wide,
  independent of view and sort.
- **Why it is safe:** every write is followed on its own thread by a `refresh_grid` that bumps
  after the commit (CLAUDE.md, the refresh chain), so a count computed after a bump has seen
  the write.
- **The rule for the future**, added to CLAUDE.md's refresh-chain paragraph beside the
  `data_dirty` one: a writer that changes a count without going through `refresh_grid` - as the
  hashing pass does - bumps `counts_epoch` itself.

### The UI

- `api.ts`: `GridInfo` loses `sections` and `folders` and gains `layout: GridLayout | null`,
  `GridLayout = { gen: number; sections: Section[]; folders: FolderTally[] }`.
  `api.gridInfo(knownLayout: number | null)`. Changed in the same commit as the Rust structs
  (CLAUDE.md: the mirror is unchecked).
- `LibraryStore.info` keeps today's shape - `sections` and `folders` on it - plus
  `layoutGen: number | null`, so `Grid.svelte`, `Viewer.svelte`, `FolderTree.svelte` and the
  timeline do not change. `loadGrid` sends `this.info.layoutGen` and takes
  `info.layout?.sections ?? this.info.sections` (and `folders`), updating `layoutGen` only when
  a layout came back. The initial `info` has `layoutGen: null`, so the first call always gets
  the layout. `lastPhoto()` needs only `len`; it passes the known generation too.
- `mock.js` (`crates/xtask/screenshots/`): `grid_info` always answers with a layout - a
  server may always send it.

## Testing

- Rust (`commands.rs`, `engine.rs`), each shown to fail with its rule reverted:
  - a star leaves `layout_gen` unchanged, and `grid_info(Some(gen))` answers `layout: None`;
  - a hide moves `layout_gen`, and `grid_info(Some(old))` returns the layout;
  - `grid_info(None)` always returns the layout;
  - a star changes `starred_count` on the next `grid_info` (the epoch bump in `data_snapshot`);
  - a hashing pass that finds a byte-identical pair changes `duplicate_count` (the bump in
    `hash_after_scan`);
  - a poster frame's derived rebuild does not recompute the counts, through a test-only
    counter of count computations.
- UI (`library.test.ts`): an answer with `layout: null` keeps the sections and folders the
  store had while taking the new `len` and counts; an answer with a layout replaces them and
  its `layoutGen`; the first call sends `null`. The `GridInfo` literals in the tests gain
  `layout`.
- A before/after timing of one `grid_info` at 5,000 folders (Rust serialize + a JSON parse of
  the payload), in the PR description, not in CI.

## Out of scope

- Deltas during a scan.
- The folder list's own fetch (`list_folders`) and the collections' (`data_changed`), already
  gated separately.
