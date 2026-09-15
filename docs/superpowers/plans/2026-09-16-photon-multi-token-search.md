# Multi-Token Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the search box's text into whitespace-delimited tokens and match them OR-wise, so `lake bell` finds `lake_bell.jpg` — which today it does not.

**Architecture:** A new `photon-core::search` module owns everything "matching" means: a `Query` type that lowercases and de-duplicates tokens once at parse time and answers `matches(&[&str])` per row. `Library::search_entries` in `crates/photon-core/src/library/items.rs` becomes a thin caller of it. Nothing else in the codebase changes — no schema, no migration, no IPC command, no TypeScript mirror, no UI.

**Tech Stack:** Rust 2024 edition, `rusqlite` (bundled SQLite), `cargo test`. No new dependency is added by this plan.

**Spec:** `docs/superpowers/specs/2026-09-16-photon-multi-token-search-design.md`

## Global Constraints

- **The Rust gate — all five commands must pass before any commit**, in this order. CI runs exactly these:
  ```bash
  cargo fmt --all
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  cargo bench -p photon-core --bench grid --no-run
  ```
- **No new dependency.** photon takes no native library dependencies, and this feature needs nothing beyond `std`.
- **Never launch the GUI to verify a change.** Verification is the test suites. This plan touches no UI, so `npm run check` and `npm test` are unaffected — but run them once at the end (Task 4) to prove that.
- **A new test must be demonstrated to fail with its change reverted.** A compile error is not that demonstration — it shows a symbol was missing, not that an assertion discriminates behaviour. Task 3 exists solely to perform that demonstration against the real, old matcher.
- **Comments carry reasoning, not mechanics.** Every comment this plan asks you to write explains *why* a line is the way it is. Do not replace them with restatements of what the code does.
- **Matching is case-insensitive in Rust, never in SQL.** There is no `COLLATE NOCASE` in this codebase and `lower()` is ASCII-only without ICU. Do not "simplify" any part of this into a SQL `LIKE`.
- Results stay in `GRID_ORDER`. No ranking, no reordering.
- Tokens are literal substrings, including punctuation-only ones (`%`, `_`, `-`). Do not filter them.

---

### Task 1: The `search` module

Creates the matcher as a standalone unit with its own tests. Nothing calls it yet — that is Task 2. This task stands alone because a reviewer can judge the semantics (OR, case folding, literal wildcards) without any database or query in the picture.

**Files:**
- Create: `crates/photon-core/src/search.rs`
- Modify: `crates/photon-core/src/lib.rs:3-14` (the `pub mod` block)

**Interfaces:**
- Consumes: nothing.
- Produces, for Task 2:
  - `photon_core::search::Query`
  - `Query::parse(raw: &str) -> Query`
  - `Query::is_empty(&self) -> bool`
  - `Query::matches(&self, haystacks: &[&str]) -> bool`

- [ ] **Step 1: Create the module file with its tests only**

Create `crates/photon-core/src/search.rs` with exactly this content. The tests come first and the implementation is empty on purpose — Step 2 runs them to watch them fail.

```rust
//! Matching a typed search query against a photo's names.
//!
//! The whole of what "search matches this photo" means lives here, rather than inside the
//! row closure in `library::items`, so that the parts that are easy to get wrong - case
//! folding outside ASCII, wildcard characters, an empty query - are testable without
//! seeding a database and counting rows.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_token_matches_a_substring() {
        // The behaviour that already shipped: one word, matched anywhere in the name.
        assert!(Query::parse("bell").matches(&["lake_bell.jpg", "Trips"]));
        assert!(Query::parse("lake").matches(&["lake_bell.jpg", "Trips"]));
    }

    #[test]
    fn every_token_is_tried_against_the_name() {
        // The case this module exists for. As one substring, "lake bell" is absent from
        // "lake_bell.jpg" - the separator is an underscore - so the old matcher missed it.
        assert!(Query::parse("lake bell").matches(&["lake_bell.jpg", "Trips"]));
    }

    #[test]
    fn one_matching_token_is_enough() {
        // Tokens are OR-ed: a half-remembered fragment does not empty the grid.
        assert!(Query::parse("lake zzz").matches(&["lake_bell.jpg", "Trips"]));
        assert!(Query::parse("zzz bell").matches(&["lake_bell.jpg", "Trips"]));
    }

    #[test]
    fn no_matching_token_is_not_a_hit() {
        assert!(!Query::parse("zzz qqq").matches(&["lake_bell.jpg", "Trips"]));
    }

    #[test]
    fn a_token_matches_the_folder_name_too() {
        // Both names are one haystack list, so a token may land in either.
        assert!(Query::parse("zzz trips").matches(&["lake_bell.jpg", "Trips"]));
    }

    #[test]
    fn blank_queries_parse_empty_and_match_nothing() {
        // `split_whitespace` handles the trimming the caller used to do by hand, and
        // agrees with `Engine::set_search_query`'s `trim().is_empty()` on what is blank.
        for raw in ["", "   ", "\t", "\n  \t "] {
            let q = Query::parse(raw);
            assert!(q.is_empty(), "{raw:?} should parse to no tokens");
            assert!(!q.matches(&["lake_bell.jpg", "Trips"]));
        }
    }

    #[test]
    fn a_non_blank_query_is_not_empty() {
        assert!(!Query::parse("lake").is_empty());
    }

    #[test]
    fn case_folds_outside_ascii() {
        // This is the test that pins "match in Rust, not with SQL LIKE": SQLite folds
        // case for ASCII only, so 'München' LIKE '%MÜNCHEN%' is false. The lowercase
        // query matches under both and so proves nothing - it is the all-caps one that
        // discriminates.
        assert!(Query::parse("MÜNCHEN").matches(&["a.jpg", "München"]));
        assert!(Query::parse("münchen").matches(&["a.jpg", "MÜNCHEN"]));
    }

    #[test]
    fn eszett_and_ss_are_not_the_same_letter() {
        // A limit of `to_lowercase`, recorded as a decision rather than left as a
        // surprise: closing it needs full case folding, which is more than this warrants.
        assert!(!Query::parse("strasse").matches(&["Straße.jpg", "Trips"]));
    }

    #[test]
    fn sql_wildcards_are_literal_characters() {
        // `contains` has no metacharacters, so a user searching for "50%" gets the
        // photos named "50%", not every photo. Punctuation-only tokens are kept for
        // exactly this reason - dropping them would break these two queries.
        assert!(Query::parse("%").matches(&["50%.jpg", "Trips"]));
        assert!(!Query::parse("%").matches(&["50.jpg", "Trips"]));
        assert!(Query::parse("_").matches(&["lake_bell.jpg", "Trips"]));
        assert!(!Query::parse("_").matches(&["lake-bell.jpg", "Trips"]));
    }

    #[test]
    fn duplicate_tokens_collapse() {
        // Re-scanning the same needle per row cannot change the answer, so parse drops
        // repeats - including ones that differ only by case.
        assert_eq!(Query::parse("lake lake LAKE").token_count(), 1);
        assert!(Query::parse("lake lake").matches(&["lake_bell.jpg", "Trips"]));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p photon-core search::`

Expected: FAIL — the compiler reports `cannot find type Query in this scope` (or `failed to resolve: use of undeclared type Query`) for every test, because `search.rs` has no `Query` yet and is not yet declared as a module.

- [ ] **Step 3: Declare the module**

In `crates/photon-core/src/lib.rs`, add `pub mod search;` to the `pub mod` list, keeping it alphabetical — between `pub mod picasa;` and `pub mod scanner;`:

```rust
pub mod picasa;
pub mod scanner;
```

becomes

```rust
pub mod picasa;
pub mod scanner;
pub mod search;
```

Note `scanner` sorts before `search` (`c` < `e`), so `search` goes after it.

- [ ] **Step 4: Write the implementation**

Insert this into `crates/photon-core/src/search.rs`, directly after the `//!` module doc comment and before `#[cfg(test)] mod tests`:

```rust
/// A parsed search query: the user's text split on whitespace into lowercased tokens,
/// any one of which matching is a hit.
///
/// **Tokens are OR-ed, so typing more words widens the result set.** That is the less
/// common choice - file managers AND, so each word narrows - and it is deliberate: the
/// query this serves is "I remember it had a lake and a bell in the name", where the user
/// is recalling fragments rather than refining a filter, and an AND punishes a
/// half-remembered fragment with an empty grid. `search_widens_with_each_added_word` in
/// `library::items` pins the decision.
///
/// **Matching is done here rather than with SQL `LIKE`** for two reasons, both of which
/// bite real libraries. SQLite folds case for ASCII only, so `MÜNCHEN` would never find
/// `München`, and `lower()` has the same limit without the ICU extension - a native
/// dependency this project does not take. And `LIKE` reads `%` and `_` in the user's text
/// as wildcards unless every one is escaped, so a search for `50%` would return
/// everything. `contains` has no metacharacters to escape and cannot get that wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    tokens: Vec<String>,
}

impl Query {
    /// Splits `raw` on whitespace and lowercases each token.
    ///
    /// `split_whitespace` does the trimming the caller would otherwise do by hand: it
    /// ignores leading, trailing and repeated whitespace, including non-ASCII whitespace,
    /// and yields nothing at all for a blank query. That keeps this in step with
    /// `Engine::set_search_query`, which decides a query is blank with `trim().is_empty()`
    /// - the two must agree, or the engine would hold a live query the matcher considers
    /// empty and the grid would go blank with text still in the box.
    ///
    /// Duplicates are dropped because re-scanning the same needle on every row cannot
    /// change the answer. The list is short enough that a linear `contains` beats
    /// building a set, and it keeps the user's order, which nothing depends on but which
    /// makes the tokens readable in a debugger.
    pub fn parse(raw: &str) -> Self {
        let mut tokens: Vec<String> = Vec::new();
        for token in raw.split_whitespace() {
            let token = token.to_lowercase();
            if !tokens.contains(&token) {
                tokens.push(token);
            }
        }
        Self { tokens }
    }

    /// Whether the query has no tokens, which is true exactly when `raw` was blank.
    ///
    /// The caller returns no rows for this rather than every row: a "search" matching the
    /// whole library is indistinguishable from the library.
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// Whether any token is a substring of any haystack, ignoring case.
    ///
    /// Each haystack is lowercased once and all tokens tried against it, rather than the
    /// other way round: `to_lowercase` allocates and the tokens are already folded, so
    /// looping tokens on the inside keeps this at one allocation per haystack, as it was
    /// when the query was a single needle. `any` short-circuits, so a hit in the file name
    /// never lowercases the folder name.
    pub fn matches(&self, haystacks: &[&str]) -> bool {
        haystacks.iter().any(|haystack| {
            let haystack = haystack.to_lowercase();
            self.tokens.iter().any(|token| haystack.contains(token))
        })
    }

    /// How many tokens the query holds. Test-only: the count is not part of what callers
    /// need, but `duplicate_tokens_collapse` has to see that de-duplication happened
    /// rather than infer it from a match that would pass either way.
    #[cfg(test)]
    fn token_count(&self) -> usize {
        self.tokens.len()
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p photon-core search::`

Expected: PASS — `test result: ok. 11 passed; 0 failed`.

- [ ] **Step 6: Run the Rust gate**

```bash
cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```

Expected: all five succeed, with no clippy warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/photon-core/src/search.rs crates/photon-core/src/lib.rs
git commit -m "feat(search): add a multi-token query matcher

Query::parse splits on whitespace and lowercases; Query::matches
answers whether any token is a substring of any name. Nothing calls it
yet.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 2: Wire the matcher into `search_entries`

Replaces the single-needle match in the grid query's row closure with the new `Query`. After this task the feature works end to end. The existing search tests in `items.rs` and `engine.rs` are all single-word queries and must stay green **unchanged** — if any of them needs editing, the change is wrong.

**Files:**
- Modify: `crates/photon-core/src/library/items.rs:1-7` (imports) and the `search_entries` function around `items.rs:522-555`

**Interfaces:**
- Consumes, from Task 1: `crate::search::Query`, `Query::parse(&str) -> Query`, `Query::is_empty(&self) -> bool`, `Query::matches(&self, &[&str]) -> bool`.
- Produces: no new public surface. `Library::entries_for(GridView::Search, query)` keeps its signature and now honours multiple tokens.

- [ ] **Step 1: Write the failing tests**

Append these two tests to the `mod tests` block in `crates/photon-core/src/library/items.rs`, directly after `fn a_query_matching_neither_name_returns_nothing()`:

```rust
    #[test]
    fn search_matches_any_word_of_a_multi_word_query() {
        // The case the single-needle matcher missed: "lake bell" is not a substring of
        // "lake_bell.jpg", because the file's separator is an underscore.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/lake_bell.jpg", 1),
                new_item(folder, "/p/mountain.jpg", 2),
            ])
            .unwrap();

        let hits: Vec<i64> = lib
            .entries_for(GridView::Search, "lake bell")
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(hits, vec![ids[0]]);
    }

    #[test]
    fn search_widens_with_each_added_word() {
        // Tokens are OR-ed, so a second word adds photos rather than removing them. This
        // is the deliberate choice the design records; an AND implementation returns one
        // row here and fails on the final assertion.
        let (_dir, lib) = temp_library();
        let (_watched, folder) = seed_folder(&lib, Path::new("/p"));
        let ids = lib
            .insert_items(&[
                new_item(folder, "/p/lake.jpg", 1),
                new_item(folder, "/p/bell.jpg", 2),
                new_item(folder, "/p/mountain.jpg", 3),
            ])
            .unwrap();

        let hits = |q: &str| -> Vec<i64> {
            lib.entries_for(GridView::Search, q)
                .unwrap()
                .iter()
                .map(|e| e.id)
                .collect()
        };

        assert_eq!(hits("lake"), vec![ids[0]]);
        assert_eq!(
            hits("lake bell"),
            vec![ids[0], ids[1]],
            "the second word adds its matches; it does not narrow the first word's"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p photon-core search_matches_any_word_of_a_multi_word_query search_widens_with_each_added_word`

Expected: both FAIL against the old matcher — `search_matches_any_word_of_a_multi_word_query` with `assertion `left == right` failed: left: [], right: [1]`, and `search_widens_with_each_added_word` on its second assertion with `left: []`. Both compile and run; this is the demonstration `CLAUDE.md` requires, and it is why these tests were written before the wiring.

- [ ] **Step 3: Import the matcher**

In `crates/photon-core/src/library/items.rs`, add the import alongside the existing `crate::` imports at the top of the file:

```rust
use super::Library;
use crate::Result;
use crate::grid::{GridEntry, GridView};
use crate::media::{MediaKind, ThumbState, fingerprint};
use crate::metadata::oriented_dims;
use crate::search::Query;
use rusqlite::{OptionalExtension, Row, params};
use std::collections::{HashMap, HashSet};
```

- [ ] **Step 4: Rewrite `search_entries`**

Replace the whole of `search_entries` — its doc comment and body — with this. The SQL, the column indices and `map_grid_row` are unchanged; only how a row is judged changes, and the reasoning that used to live in this comment has moved to `search.rs` alongside the code it justifies.

```rust
    /// Photos whose file name or folder name contains any word of `query`,
    /// case-insensitively.
    ///
    /// The words are OR-ed and the matching runs in Rust rather than as SQL `LIKE`;
    /// `search::Query` holds both decisions and the reasons for them. This is one pass
    /// over the same rows an index rebuild already reads, with two short string compares
    /// per token added per row.
    fn search_entries(&self, query: &str) -> Result<Vec<GridEntry>> {
        let query = Query::parse(query);
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.reader()?;
        let mut stmt = conn.prepare(&grid_query(
            &format!("{GRID_COLUMNS}, i.file_name, f.name"),
            "",
        ))?;
        let rows = stmt
            .query_map([], |r| {
                let file_name: String = r.get(GRID_COLUMN_COUNT)?;
                let folder_name: String = r.get(GRID_COLUMN_COUNT + 1)?;
                let hit = query.matches(&[&file_name, &folder_name]);
                // No `Ok(…?)` wrapper here: the closure already returns this type, and
                // wrapping it trips `clippy::needless_question_mark`, which the gate
                // treats as an error.
                hit.then(|| map_grid_row(r)).transpose()
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        Ok(rows)
    }
```

- [ ] **Step 5: Run the new tests to verify they pass**

Run: `cargo test -p photon-core search_matches_any_word_of_a_multi_word_query search_widens_with_each_added_word`

Expected: PASS — `test result: ok. 2 passed; 0 failed`.

- [ ] **Step 6: Verify every pre-existing search test still passes, untouched**

Run: `cargo test -p photon-core search`

Expected: PASS, including `search_matches_a_substring_of_the_file_name`, `search_matches_a_substring_of_the_folder_name`, `a_query_matching_neither_name_returns_nothing`, `search_folds_case_for_non_ascii_text`, `search_does_not_treat_ss_and_eszett_as_the_same_letter`, `search_treats_sql_wildcards_as_literal_characters`, `search_keeps_grid_order_and_excludes_missing_items` and `an_empty_search_query_matches_nothing_rather_than_everything`.

If you had to edit any of these to make them pass, stop: single-word behaviour is supposed to be identical, so an edit means the new matcher changed something it should not have.

- [ ] **Step 7: Verify the engine-level search tests still pass**

Run: `cargo test -p photon-app search`

Expected: PASS — `setting_a_search_query_switches_to_the_search_view_and_filters_the_grid`, `an_empty_search_query_returns_to_the_all_view` and `switching_to_another_view_clears_the_search_query`.

- [ ] **Step 8: Run the Rust gate**

```bash
cargo fmt --all
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```

Expected: all five succeed.

- [ ] **Step 9: Commit**

```bash
git add crates/photon-core/src/library/items.rs
git commit -m "feat(search): match any word of the query

\"lake bell\" now finds lake_bell.jpg. search_entries hands both names
to search::Query instead of testing one substring; single-word queries
behave exactly as before, so the existing search tests are untouched.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 3: Prove the tests discriminate

`CLAUDE.md` requires a new test to be demonstrated to fail with its change reverted, and warns that tests passing with and without the change have shipped here before. Step 2 of Task 2 did this for the two integration tests. This task does it for Task 1's unit tests, which could not fail for the right reason at the time they were written — the type did not exist yet, so they failed to compile rather than failing an assertion.

Nothing is committed by this task. It is a verification you run and record.

**Files:**
- Temporarily modify: `crates/photon-core/src/search.rs` (reverted before the task ends)

**Interfaces:**
- Consumes: Task 1's `Query`.
- Produces: nothing.

- [ ] **Step 1: Revert the matcher to single-needle behaviour**

In `crates/photon-core/src/search.rs`, temporarily replace the body of `parse` so the whole query is one token, which is what shipped before this work:

```rust
    pub fn parse(raw: &str) -> Self {
        let raw = raw.trim().to_lowercase();
        let tokens = if raw.is_empty() { Vec::new() } else { vec![raw] };
        Self { tokens }
    }
```

- [ ] **Step 2: Run the unit tests and record which fail**

Run: `cargo test -p photon-core search::`

Expected: FAIL. Specifically `every_token_is_tried_against_the_name`, `one_matching_token_is_enough`, `a_token_matches_the_folder_name_too` (its query is two words) and `duplicate_tokens_collapse` all fail on their assertions — not on compilation. The others pass, correctly: they pin behaviour this change was not supposed to alter.

If any of those four **passes** under the reverted `parse`, the test does not discriminate and must be strengthened before you go on.

- [ ] **Step 3: Restore the real implementation**

```bash
git checkout crates/photon-core/src/search.rs
```

- [ ] **Step 4: Confirm the restore**

Run: `cargo test -p photon-core search::`

Expected: PASS — `test result: ok. 11 passed; 0 failed`.

Run: `git status --short`

Expected: no modified files.

---

### Task 4: Full-gate verification

The feature touches no UI and no TypeScript, which is itself a claim worth checking rather than assuming.

**Files:**
- None modified.

**Interfaces:**
- Consumes: the finished feature.
- Produces: nothing.

- [ ] **Step 1: Confirm nothing outside photon-core changed**

Run: `git diff --stat main...HEAD`

Expected: only `crates/photon-core/src/search.rs`, `crates/photon-core/src/lib.rs`, `crates/photon-core/src/library/items.rs`, and the two `docs/superpowers/` files. No `ui/`, no `crates/photon-app/`, no `capabilities/default.json`, no `schema.rs`.

- [ ] **Step 2: Run the Rust gate one final time**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p photon-core --bench grid --no-run
```

Expected: all four succeed.

- [ ] **Step 3: Run the UI gate**

```bash
npm run check
npm test
```

Expected: `svelte-check` reports 0 errors and 0 warnings, and vitest passes. Neither should have changed — this is the check that the claim "no UI change" is true.

- [ ] **Step 4: Report**

State plainly which commands were run and their results. Do not claim the feature works without having run them.

---

## Notes for the implementer

- **Do not add an FTS5 table or any index.** `LIKE '%foo%'` cannot use a B-tree index, and an FTS table would be a second source of truth to keep in sync on every scan, rename and removal. The design spec §2 and the parent spec §2 both reject it.
- **Do not filter punctuation-only tokens.** It looks like an improvement — `lake - bell` would stop matching every hyphenated name — but it breaks `search_treats_sql_wildcards_as_literal_characters`, an existing deliberate decision that a query of `%` or `_` matches those characters. Literalness wins.
- **Do not sort results by match count.** The sidebar groups by the same value the grid orders by, so ranking would put the two on different axes. `CLAUDE.md` calls this out specifically.
- **Do not touch `ScanReport` or `refresh_grid`.** This change alters no rows and no data; it only changes which existing rows a view selects, and the refresh chain already rebuilds the index when the query changes.
