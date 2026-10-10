# The native search box: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** the third of sub-project 2's five pull requests: the search box in the top bar,
searching as it is typed; Enter, Escape, Ctrl+F and `/`; the help that lists the grammar;
the bookmark that saves a search.

**Architecture:** `search_box.rs` is the Svelte `createSearchBox` on a clock that is a
number: the text, the send that is due, and what a view switch does to both. `search_help.rs`
is the grammar's list and `insertTerm`, held to the parser by the test the Svelte list has.
`search_bar.rs` is the view: egui's text field, the two buttons and the panel. A typed
search is a `Step::Search` on the views' queue, where it takes the place of a search still
waiting. Saving is a write to the library, made on a queue of its own.

**Spec:** `docs/superpowers/specs/2026-10-10-photon-native-sidebar-views-search-design.md`,
"The search", and the shell's "Who has the keys".

Written with the user away, as the second pull request's was: interfaces and tests, not
the code.

## Global constraints

As the second pull request's plan: no egui type in a state module, each added to
`STATE_MODULES`; nothing reads or writes SQLite on the UI thread; every new test shown to
fail with its rule broken; the Rust gate at every commit; every string the Svelte UI's;
the application never launched; a view setter is called through the queue alone.

## Left out, and to where

- The text field's own limits - text that mixes writing directions is not reordered, and
  an input method cannot be tried without a window - are the spec's first risk: on the
  smoke checklist, not fixed.
- The panel closing when the focus walks out of the field by Tab: the Svelte box does; the
  spec names Escape, a press outside and a pick. Left as the spec has it.
- `keys.rs` and the sheet that lists the keys: sub-project 6. The three keys answered here
  are listed in the pull request.

## Review focus

1. **A typed search landing behind a view switch.** A click on Starred, a folder or All
   photos within the debounce must not be followed by the search that was half typed.
2. **The box and the grid disagreeing.** After a switch away the box is empty; after a
   refused switch it has its text back unless the user typed on; after a refused search it
   shows what the grid shows, unless the user typed on.
3. **Escape throwing the user out of a view.** An empty box outside a search clears
   nothing: clearing sends the empty query, which is All photos.
4. **Keys meant for the field reaching the grid, and the reverse.** Home, End and the
   arrows in the field; `/` typed into a name; a held `/` typing a second one.
5. **A still window.** A due send asks for its own frame and for none after it.

## Task 1: `search_box.rs` and `search_help.rs`

**Produces:**

```rust
// search_box.rs
pub const SEARCH_DEBOUNCE_MS: f64 = 150.0;
#[derive(Default)] pub struct SearchBox { pub text: String, /* due, left */ }
impl SearchBox {
    pub fn typed(&mut self, now_ms: f64);
    pub fn due_at(&self) -> Option<f64>;
    pub fn take_due(&mut self, now_ms: f64) -> Option<String>;
    pub fn cancel(&mut self);
    pub fn search(&mut self, query: &str);
    pub fn clear(&mut self);
    pub fn leave(&mut self, step: u64);
    pub fn refused(&mut self, step: u64, asked: Option<&str>, shown: &str);
}
pub fn saved_search_for<'a>(searches: &'a [SavedSearch], query: &str) -> Option<&'a SavedSearch>;
pub fn can_save_search(searches: &[SavedSearch], query: &str) -> bool;
// search_help.rs
pub struct Entry { pub text: &'static str, pub insert: bool, pub does: &'static str }
pub struct HelpGroup { pub title: &'static str, pub entries: &'static [Entry] }
pub const SEARCH_HELP: &[HelpGroup];
pub fn insert_term(query: &str, term: &str) -> String;
```

- [ ] Tests first: `search-box.svelte.test.ts`'s nine cases on a numeric clock;
  `searches.test.ts`'s cases for the two functions; `search-help.test.ts` whole, reading
  `Query::terms` out of `photon-core`'s `search.rs` by line (a Windows checkout is CRLF).
- [ ] Implement, add both to `STATE_MODULES`, gate, commit.

## Task 2: `nav.rs` says which step was refused

`Nav::answered` answers the step with what the engine said, so that the box can tell a
refused search from a refused switch.

- [ ] Test first, implement, `app.rs` follows, gate, commit.

## Task 3: `search_bar.rs`, the view

**Produces:**

```rust
pub struct SearchBarData<'a> { pub saved_as: Option<&'a str>, pub can_save: bool, pub in_search: bool }
pub enum SearchAction { Typed, Clear, Save }
#[derive(Default)] pub struct SearchBar { /* help open, the slash that is held */ }
impl SearchBar { pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, text: &mut String, data: &SearchBarData<'_>) -> Vec<SearchAction>; }
```

Two icons more: search, circle-help.

- [ ] Tests first, in whole frames: typing changes the text and answers `Typed`; Enter
  leaves the field and answers nothing; Escape clears a box with text, clears an empty
  one inside a search, and leaves an empty one outside a search without clearing; Escape
  with the help open closes the help and nothing else; Ctrl+F and `/` focus the field with
  its text selected, `/` only while no field has the keys, and a held `/` types nothing;
  the help opens under the box, an entry that inserts puts its term in the text with an
  open quote closed and answers `Typed`, one that does not is not a button, a press
  outside closes it; the bookmark is there only with text, saves when it can, and is
  filled and takes no press once saved; the placeholder shows in an empty box.
- [ ] Implement; the shell draws it in the top bar. Gate, commit.

## Task 4: the application

- A due search is asked for with `Queue::push_or_replace`, in the place of a search still
  waiting; the frame it is due in is asked for.
- A row's step empties the box (`leave`) or fills it (`search`); a folder click cancels
  what is pending; a refused step puts back what the box needs.
- Saving is a job on a queue of writes; when it is done the collections are read again,
  and a refusal is a toast.

- [ ] Tests first, through the application: a typed search is shown after the debounce
  and not before; a click on a view within the debounce is not followed by the search;
  the box is empty after a switch and filled by a saved search's row; Escape in a search
  goes back to All photos, and in Starred with an empty box goes nowhere; the bookmark
  saves and the sidebar lists it; a still window with a search shown draws no frame.
- [ ] Implement, gate, commit.

## Task 5: pictures, the checklist, CLAUDE.md, the pull request

- [ ] A picture of a search with its help open; the checklist's items, the field's two
  limits among them; CLAUDE.md; push; one fresh reviewer; one fix pass; merge.
