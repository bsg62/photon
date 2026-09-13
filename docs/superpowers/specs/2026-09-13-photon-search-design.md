# photon — Search Design

**Date:** 2026-09-13
**Status:** Approved design, pending implementation plan
**Parent spec:** `docs/superpowers/specs/2026-09-11-photon-v1-library-viewer-design.md`
**Builds on:** the starred-photos work (`d16a955`), whose view mechanism this extends

## 1. Scope

Type into a box, see the photos whose **file name** or **folder name** contains what you typed. Nothing more.

### In scope

- Substring matching against `items.file_name` and `folders.name`.
- Case-insensitive matching that works for non-ASCII text (§3).
- A third grid view, `Search`, alongside `All` and `Starred`.
- A search box in the sidebar, and an empty state when nothing matches.

### Out of scope

- EXIF, camera, date, dimension or rating terms; ranking and relevance ordering; fuzzy or prefix-expanded matching; query syntax of any kind (quotes, `AND`, `-foo`); search history; a full-text index.

Every one of those is a reason to have built an FTS table. This version deliberately does not, because none of them is asked for, and the data to answer them is already in memory or one scan away.

### The invariant stands

> photon never writes to, moves or deletes files inside watched folders.

Search reads. Nothing here touches a photo.

## 2. No schema change

`items.file_name` and `folders.name` already exist, both denormalised, and `grid_entries_for` already joins `folders`. The columns this feature matches against are the ones the grid query is already reading.

**No FTS5 table, and no new index.** An FTS index would have to be kept in sync with every scan, every rename and every removal — a second source of truth, and a staleness surface — to speed up a scan that §3 shows costs the same as work photon already does on every library change. `LIKE '%foo%'` cannot use a B-tree index either, so an index would buy nothing even without FTS.

This is therefore the first feature since the library existed that adds no column and runs no migration. `user_version` stays at 2.

## 3. Matching happens in Rust, not in SQL

The obvious implementation is `WHERE i.file_name LIKE ?1 OR f.name LIKE ?1`. **It is rejected**, for two reasons that both bite this user's library specifically:

1. **SQLite's `LIKE` folds case for ASCII only.** Searching `MÜNCHEN` would not find `München/`: `M`↔`m` folds as ASCII, but `Ü` never folds to `ü`. `lower()` has the same limit without the ICU extension, which is a native dependency photon does not take (§ packaging constraint). Rust's `to_lowercase` is Unicode-aware and already available.

   **The lowercase query is the trap.** `'München' LIKE '%münchen%'` returns **1** — verified against SQLite, not assumed. Anyone testing this decision with the obvious example finds `LIKE` working and concludes the Rust matcher is unjustified overhead. It is the all-caps query that silently fails, which is exactly the kind of half-working behaviour that survives a casual check and reaches a user.
2. **`LIKE` forces escaping.** A query containing `%` or `_` is a wildcard unless every one is escaped with an `ESCAPE` clause. A user typing `50%` gets every photo. Substring matching in Rust has no metacharacters to escape and so cannot get this wrong.

So the query selects the same columns as the grid query plus `i.file_name` and `f.name`, and the filter is `haystack.to_lowercase().contains(&needle)` evaluated per row, with the needle lowercased once.

**The cost is acceptable and bounded.** This is one pass over the same rows `grid_entries` already reads to build the index at startup and after every scan — the spec's sub-second budget for 100k items covers exactly this shape of work. Search adds a lowercase and a substring scan per row against two short strings. It does not read files, decode images or touch thumbnails.

If search ever grows terms that a scan cannot answer cheaply, that is the moment to reconsider an index — not before.

## 4. Search is a third view

`GridView` gains a `Search` variant, and `Engine` gains the query string beside it:

```rust
view: RwLock<GridView>,
search_query: RwLock<String>,
```

**The query does not live inside the enum.** `GridView` is `Copy`, is serialised as a bare string (`'all' | 'starred'`), and is mirrored in TypeScript as a union of string literals. A `Search(String)` variant would make it non-`Copy` and turn its wire form into a tagged object, changing every existing use to buy nothing — the query is one more piece of engine state, and it belongs beside the view rather than inside it.

`refresh_grid` already reads `*self.view.read()` and rebuilds. It reads the query too, and passes both to one entry point:

```rust
pub fn entries_for(&self, view: GridView, query: &str) -> Result<Vec<GridEntry>>
```

**One function, not two.** `grid_entries_for(view)` matches exhaustively on the enum, so a `Search` variant would leave it with an arm it cannot serve — either an `unreachable!` or a silently wrong result. Taking the query alongside the view means every variant is answerable, at the cost of two arms ignoring a parameter. The engine holds both pieces of state at every call site anyway.

`All` and `Starred` keep the existing filtered SQL; `Search` runs the variant that also selects `i.file_name` and `f.name` and filters in Rust. **Both map a row to a `GridEntry` through one shared function**, so the two paths cannot drift — a row mapping duplicated across query paths is exactly how the starred work nearly shipped every photo unstarred. Both return `GRID_ORDER` and are otherwise indistinguishable to everything downstream.

**Everything downstream is unchanged**, exactly as it was for Starred: paging, folder sections, viewer navigation, neighbour preloading and the position-in-folder counter all work because the index is simply a different set of rows.

**An empty or whitespace-only query is not a search.** It switches the view back to `All` rather than matching everything, so clearing the box returns to the library instead of leaving a "search view" that is indistinguishable from it but labelled differently.

## 5. The UI

A text input sits in the sidebar toolbar, above the Starred row.

- Typing sets the query and switches to the Search view, **debounced by 150 ms**, so a five-letter word costs one index rebuild rather than five.
- Clearing it returns to `All`.
- Escape in the box clears it. The viewer's own Escape handling is unaffected — it only ever sees keys when it is open, and it is not open while the sidebar has focus.
- Clicking a folder leaves the Search view first, awaiting the switch before jumping, exactly as `jumpToFolder` already does for Starred. **That await is load-bearing**: the jump resolves an offset against the grid's current index, and racing it against an unawaited view switch returns a stale offset. This is the defect the starred-photos review caught, and the same shape reappears here.

**The grid returns to the top when results change.** `App.svelte` already resets scroll on view change, but that effect keys on `view` alone — refining a query while staying in the Search view would leave the scroll position from the previous, larger result set. The effect must therefore key on the query as well as the view.

**When nothing matches**, the grid shows "No photos match <query>" rather than an empty expanse, which is indistinguishable from a bug.

`GridInfo` carries `search_query` so the UI can render the active state from one source of truth rather than tracking it locally, and its TypeScript mirror in `ui/src/lib/api.ts` is updated in the same task as the Rust struct — an unmirrored field is silently `undefined` at runtime, because structural interfaces never validate incoming JSON.

## 6. Error handling

- **A failed search query**: propagates, and is surfaced through the store's existing error path. The index is rebuilt and published only on success, so a failed query leaves the previous results on screen alongside the error rather than replacing them.

  **The engine's declared view and query roll back too.** Without that, a failed rebuild would leave the engine reporting `Search` with the new query while the grid still held the old photos — and §5 makes `GridInfo.search_query` the single source of truth the UI renders from, so the box would show a query whose results are not on screen. Two further consequences make it worse than a momentary glitch: the state is *sticky*, because every later rebuild (a scan, the watcher) re-reads the same failing query and fails again, so the grid never updates until the user changes something; and the empty state cannot rescue it, because `len` is still the previous non-zero count, so "No photos match" never renders and the full library is displayed beneath a search box containing the query. The rollback is what keeps §5's "one source of truth" true when §6's path is taken.

  This deliberately does **not** follow `starred_count`, which logs and falls back to `0`. That fallback is right for a sidebar count, where a wrong number degrades benignly. It is wrong for the grid: an empty result is indistinguishable from "nothing matched", so a broken query would look like a successful search for something absent — the confusion §5's empty state exists to prevent.
- **No matches**: not an error. §5's empty state.
- Nothing else here can fail: there is no new I/O, no new file access, and no migration.

## 7. Testing

- **The matcher**, as unit tests over a seeded library: a filename substring matches; a folder-name substring matches; a query matching neither returns nothing; matching is case-insensitive **for non-ASCII** (`MÜNCHEN` finds `München` — the case `LIKE` gets wrong; `münchen` matches under both and so proves nothing) — this test is the one that pins §3's whole argument, and it fails against a `LIKE` implementation; **`ß` and `ss` are not the same letter** (`strasse` does not find `Straße.jpg`), which is a limit of Rust's `to_lowercase` rather than a bug, recorded as a test so it is a decision rather than a surprise — fixing it needs full case-folding, which is more than a simple search warrants; a query containing `%` and `_` matches those characters literally rather than acting as a wildcard; results keep `GRID_ORDER`; missing items stay excluded.
- **Empty query**: returns the view to `All`.
- **The view plumbing**: setting a query and reading `GridInfo` back reports the query and the `Search` view; switching to another view clears results.
- **UI**: the debounce fires once per pause rather than per keystroke; clearing the box restores the All view; scroll resets when the query changes within the Search view.
- **Manual checklist**: typing a folder's name finds its photos; the count and sections look right; clicking a folder in the sidebar leaves search and lands on that folder.

## 8. Success criteria

- Typing part of a file or folder name shows the matching photos, and clearing the box restores the library.
- A query in German with umlauts matches regardless of case, including an all-caps one.
- Searching a 100k library is fast enough not to feel like a mode change — the same bar Starred had to clear, and the same work.
- No new table, column, index or migration.
