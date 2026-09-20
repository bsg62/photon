# Keywords on a selection

## Why

Grid multi-select (v0.18.0) gave the context menu three verbs: star, unstar and file into an
album. Keywords were left per photo — `add_item_tag`/`remove_item_tag` take one id and live
only in the viewer's info panel — so tagging twelve photos means opening twelve photos. This
adds the missing verb, in the shape the other two already have.

## What the user sees

Right-click a selection (or one tile) in the grid: two new entries under the star verbs,
**Add keyword…** and **Remove keyword…**. Each opens a small dialog over the grid:

- A text field, the same one the info panel uses, focused on open.
- Under it, the library's existing keywords, narrowed as you type, each a button. The list is
  what makes the dialog worth having over a bare prompt: it is how you avoid `Beach` when you
  already have `beach`.
- Enter, or clicking a keyword, applies it to the whole selection and closes.
- Escape closes without writing. A blank field does nothing.

A toast reports what landed: *Added “beach” to 12 photos*, *Removed “beach” from 12 photos*.

The dialog is titled for the mode and names the count it will act on, because the selection
is not visible behind it once the dialog has focus.

## What it does not do

- **It does not write to the photo files.** Keywords are read from the file and never written
  (Conventions); an added keyword is an `item_user_tags` row, a removed one is a suppression
  row, exactly as the per-photo path already does. The dialog changes how many rows are
  written at once, not what is written.
- **No new selection semantics.** It acts on `library.selected`'s ids, like the star verbs,
  through the same `withSelection`.
- **No per-selection counts.** Showing "in 5 of 12" needs a grouped read over an id list,
  which the library has no shape for (every bulk write here is a per-id loop in one
  transaction, and there is no `IN`-list idiom to borrow). It is worth having and is not worth
  inventing a temp-table read for now; the suggestion list answers the question the user
  actually has, which is what the *library* calls this keyword.

## Behaviour that follows from the existing rules

`Library::add_item_tag` already resolves a rename rule (a name the user renamed away stores as
its target) and drops a *removal* rule for the typed name. Those are global effects, already
documented and tested for one photo; applying the same call to n photos must resolve the name
**once**, not n times, so a rule changing under a long write cannot split one click across two
names. Hence the bulk call does the name resolution and the rule delete once, then loops the
insert.

`remove_item_tag` deletes the user's added rows and suppresses the file's own keywords that
show under that name. The bulk form loops both statements per id in one transaction.

Both refresh the grid once for the batch, not once per photo — the Tag view and the sidebar's
counts read from the index, and n rebuilds of a 50,000-row index is the difference between
instant and a stall.

Unknown or missing ids are skipped rather than fatal, and the count returned is what landed:
a selection can outlive the photos in it (`set_stars` decided this and said why).

## Surface

Rust, per IPC's three files:

```
Library::add_items_tag(&[i64], &str) -> Result<(String, usize)>   # name stored, rows written
Library::remove_items_tag(&[i64], &str) -> Result<usize>
```

`add_item_tag`/`remove_item_tag` become their one-element cases, the way `picasa::set_star`
became `set_stars`': one writer, so the two cannot drift.

```
Engine::add_items_tag / remove_items_tag   # live-id filter, one refresh_grid
commands::add_items_tag -> TagWrite { tag, count }   # + ipc.rs wrapper + app.rs handler
```

TypeScript mirrors `TagWrite` in `api.ts` by hand, as always.

UI: `createTagPicker` in `lib/tag-picker.svelte.ts` holds everything testable — the query, the
narrowed suggestions, what Enter would apply, the busy flag that stops a double submit — and
`TagPicker.svelte` is the markup and the focus wiring, checked by `svelte-check` and the smoke
checklist. Matching is case-insensitive **in TypeScript**, since the list is already in hand.

## Tests

- `tags.rs`: a bulk add over three photos writes three rows under one resolved name; a bulk
  add of a renamed-away name stores the target once; a bulk remove suppresses the file's own
  keyword on every id; an unknown id among live ones does not stop the rest.
- `engine.rs`: one `library_changed` for a batch, not n.
- `tag-picker.svelte.test.ts`: narrowing is case-insensitive; an exact match sorts first;
  Enter on a blank query does nothing; a second Enter while the first is in flight does not
  write twice.
