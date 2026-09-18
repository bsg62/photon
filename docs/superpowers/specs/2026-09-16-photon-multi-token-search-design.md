# photon — Multi-Token Search Design

**Date:** 2026-09-16
**Superseded in part:** the OR rule, by `2026-09-18-photon-search-grammar-design.md`
**Status:** Approved design, pending implementation plan
**Parent spec:** `docs/superpowers/specs/2026-09-13-photon-search-design.md`, whose matcher this
replaces and whose §1–§6 otherwise stand unchanged

## 1. What changes, and why

Search today is a single case-insensitive substring match: the whole trimmed query is one
needle, tested against `items.file_name` and `folders.name` (`items.rs`, `search_entries`).

So `lake` finds `lake_bell.jpg`, and `bell` finds it too. **`lake bell` finds nothing** — the
space is part of the needle and the file's separator is an underscore. A user who remembers two
things about a photo and types both gets fewer results than a user who remembers one, which is
the opposite of what typing more words should do.

This spec splits the query into whitespace-delimited tokens and matches them **OR**-wise: a row
is a hit if *any* token is a substring of either name.

### In scope

- Whitespace tokenization of the query.
- OR combination across tokens.
- Extracting the matcher into its own unit so its semantics can be tested without a database.

### Out of scope

Quoted phrases, `-exclusion`, an explicit `AND`, relevance ranking, prefix expansion, an FTS
index. The parent spec §1 rules these out and nothing here asks for them.

### The invariant stands

> photon never writes to, moves or deletes files inside watched folders.

Search reads. Nothing here touches a photo.

## 2. OR, not AND

**Typing more words widens the result set.** `lake bell` returns everything matching `lake`
plus everything matching `bell`, including photos matching only one of them.

This is a deliberate choice and the less common one — file managers and photo apps generally
AND, so that each added word narrows. It is chosen here because the query this feature exists
to serve is *"I remember it had a lake and a bell in the name"*: the user is recalling
fragments, not refining a filter, and an AND that requires every fragment to be right punishes
a half-remembered one with an empty grid.

The cost is that a query where one token is very common — `img`, `dsc`, a hyphen — is dominated
by that token no matter what else is typed, and that a second word can never be used to cut
down a too-large result set. `search_widens_with_each_added_word` records the decision as a
test, so a future reader finds a choice rather than a bug.

**Order is not part of this.** Results stay in `GRID_ORDER`. Ranking rows by how many tokens
they matched would put the grid on a different axis than the sidebar, which groups by the same
value the grid orders by — `CLAUDE.md` calls that split out specifically. A photo matching both
tokens therefore appears where its folder puts it, not first.

## 3. Tokens are literal substrings

Each token is matched with `contains`, exactly as the whole query is today. A token does not
have to align with a separator in the file name: `lake` matches `lake_bell.jpg`,
`snowflake.jpg` and `Lakeside/` alike.

The alternative — splitting the *file name* on `_ - . space` and requiring a token to equal a
whole part — gives tighter results, but it makes a prefix stop matching: `lak` would find
nothing while the user is still typing, and search here runs on every pause.

**Punctuation-only tokens are kept.** `lake - bell` is three tokens, and the `-` matches every
hyphenated name. Dropping tokens with no alphanumeric character would fix that, and it would
also break the parent spec's deliberate rule that a query containing `%` or `_` matches those
characters **literally** rather than as SQL wildcards — `search_treats_sql_wildcards_as_literal_characters`
searches for exactly those single-character queries. Literalness is load-bearing and the wart
is not; the wart gets a comment.

## 4. The matcher moves into `photon-core::search`

A new module, `crates/photon-core/src/search.rs`, holds the whole of what "matching" means:

```rust
/// A parsed search query: the user's text split on whitespace into lowercased tokens,
/// any one of which matching is a hit.
pub struct Query {
    tokens: Vec<String>,
}

impl Query {
    pub fn parse(raw: &str) -> Self;
    pub fn is_empty(&self) -> bool;
    pub fn matches(&self, haystacks: &[&str]) -> bool;
}
```

- `parse` lowercases each token with Rust's Unicode-aware `to_lowercase` and drops duplicates,
  which cannot change the answer and would otherwise be re-scanned per row.
- `split_whitespace` subsumes the current `.trim()`: it collapses runs of whitespace, handles
  non-ASCII whitespace, and yields nothing at all for a whitespace-only query.
- `matches` lowercases each haystack **once** and loops the tokens inside it, short-circuiting
  on the first hit — so a row still costs at most two `to_lowercase` calls, as it does today.

**Why a module rather than eight lines inside the row closure.** Every interesting thing about
this feature is a property of the matcher — Unicode case folding, wildcard literalness, empty
queries, duplicate tokens, OR — and today each of those can only be tested by seeding a SQLite
library and counting returned rows. As its own unit it is testable directly, and the closure in
`search_entries` goes back to having one job.

The parent spec §3's argument for matching in Rust rather than with SQL `LIKE` (SQLite folds
case for ASCII only, so `MÜNCHEN` misses `München`; `LIKE` reads `%` and `_` as wildcards) moves
into this module's doc comment, next to the code it justifies.

## 5. The call site

`search_entries` keeps its query — the same `grid_query` selecting `GRID_COLUMNS` plus
`i.file_name` and `f.name` — and changes only how a row is judged:

```rust
let query = search::Query::parse(query);
if query.is_empty() {
    return Ok(Vec::new());
}
// ...per row:
let hit = query.matches(&[&file_name, &folder_name]);
```

`map_grid_row`, `GRID_ORDER` and the `missing_since IS NULL` exclusion are untouched, so
paging, folder sections, viewer navigation and neighbour preloading are untouched: this is
still the same view returning a different set of rows.

**Nothing else moves.** No schema change and no migration (`user_version` stays where it is).
No new IPC command, so `commands.rs`, `ipc.rs`, `app.rs` and `capabilities/default.json` are
not involved. No change to `ui/src/lib/api.ts`, because no serde struct gains a field. No UI
change at all: the box already sends whatever was typed, and `Engine::set_search_query`'s
`query.trim().is_empty()` check agrees with `split_whitespace` on exactly which queries are
empty.

## 6. Cost

Matching becomes O(rows × tokens) where it was O(rows). The constant is a `contains` over two
short strings that are already lowercased once per row, and a real query is one to four words.
Against the pass `GridIndex::build` already makes over the same rows — and behind the existing
150 ms debounce, which means one pass per typing pause rather than one per keystroke — this is
not a cost worth designing around. A pasted paragraph is the pathological case, and token
count is otherwise uncapped; what bounds it is that `entries_for` runs off the engine's grid
lock (`crates/photon-app/src/engine.rs`, `refresh_grid`, around line 182 — the query and the
rebuild run with no view or query lock held), so a huge paste degrades search latency rather
than freezing the app.

If search ever grows terms a scan cannot answer cheaply, that is the moment to reconsider an
index — as the parent spec said, and it is still not now.

## 7. Error handling

Unchanged from the parent spec §6. `Query::parse` cannot fail: there is no syntax to reject, so
no query is malformed. A failing *SQL* read still propagates, and the engine still rolls its
declared view and query back so `GridInfo.search_query` keeps matching what is on screen.

An empty result is still not an error — it is §5's "No photos match" empty state, which now
requires every token to miss.

## 8. Testing

**Unit tests in `search.rs`**, with no database:

- a single token still substring-matches, so `bell` finds `lake_bell.jpg` as it does today;
- `lake bell` matches `lake_bell.jpg` — the case that fails today, and the reason for this spec;
- a query where only one token matches is a hit (the OR);
- a query where no token matches is not;
- a token matches against the folder name as well as the file name;
- `""`, `"   "` and a tab-only query all parse empty and match nothing;
- `MÜNCHEN` matches `München` — the case SQL `LIKE` gets wrong;
- `strasse` does not match `Straße.jpg`, a recorded limit of `to_lowercase` rather than a bug;
- `%` and `_` match those characters literally;
- duplicate tokens collapse, and a repeated word does not change the result.

**Integration tests in `items.rs`**, through a seeded library:

- `lake bell` returns `lake_bell.jpg`, in `GRID_ORDER`, excluding missing items;
- `search_widens_with_each_added_word` — a second word adds rows rather than removing them,
  pinning §2.

The parent spec's existing search tests in `items.rs` and `engine.rs` stay green **unchanged**:
every one of them is a single-word query, and single-word behaviour is identical.

Per `CLAUDE.md`, each new test is demonstrated to fail with the change reverted — a compile
error does not count as that demonstration, so the integration tests are the ones run against
the old matcher.

No manual smoke-checklist entry: there is no new UI and nothing here needs eyes.

## 9. Success criteria

- Typing `lake bell` finds `lake_bell.jpg`; so does `lake`, and so does `bell`.
- Typing a second word never returns fewer photos than the first word alone.
- Results stay in `GRID_ORDER`, and the sidebar still indexes the grid.
- Every existing search test passes untouched.
- No new table, column, index, migration, IPC command or TypeScript mirror.
