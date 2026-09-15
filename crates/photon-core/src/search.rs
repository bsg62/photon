//! Matching a typed search query against a photo's names.
//!
//! The whole of what "search matches this photo" means lives here, rather than inside the
//! row closure in `library::items`, so that the parts that are easy to get wrong - case
//! folding outside ASCII, wildcard characters, an empty query - are testable without
//! seeding a database and counting rows.

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
    /// `Engine::set_search_query`, which decides a query is blank with `trim().is_empty()` -
    /// the two must agree, or the engine would hold a live query the matcher considers
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
    /// need, but `duplicate_tokens_collapse` has to see that de-duplication happened,
    /// since a variant that kept duplicate tokens would still pass that test's match
    /// assertion and differ only in the count.
    #[cfg(test)]
    fn token_count(&self) -> usize {
        self.tokens.len()
    }
}

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
