# photon — Setting Tags Design

**Date:** 2026-09-18
**Status:** Approved design
**Builds on:** v0.14.2

## 1. What changes

Tags today are read only. They are the keywords photon reads from each photo (XMP
`dc:subject`, IPTC 2:25) into `item_tags`, and the only way to change them is the tag
manager in Settings, which renames or removes a tag across the whole library.

This design lets the user **set tags on one photo**, from the viewer's info panel: add a
tag by typing it, remove one with an `×`. Both file keywords and the user's own tags are
removable per photo.

### The promise is unchanged

photon never writes photo files. A per-photo tag change is a **row in `library.db`**,
exactly as a rename rule is. The files keep their keywords and other applications see them
unchanged.

### Portability, deliberately deferred

Tags set in photon do not travel with the photo. The alternative — writing XMP
`dc:subject` back into the file — was considered and rejected for now. Pure-Rust XMP
crates exist (`gufo-xmp`, `xmpkit`, `xmp-writer` over `img-parts`), so the native-dependency
rule is not what blocks it; the blocker is that writing means **rewriting the user's photo
file in place**, against originals that may have no backup, and unevenly across the four
formats photon supports.

An **opt-in XMP export**, a deliberate action on chosen photos, remains open as a separate
design. This schema is shaped so that export can be written later: `item_user_tags` plus
`tag_rules` is the complete record of how a photo's tags differ from its file, so an export
pass can derive everything it needs. No sync or dirty column is added now — that would be
schema written against a feature that does not exist.

### Out of scope

Writing keywords to the photo or to `.picasa.ini`; tagging a batch of photos (the grid's
selection is one photo, and multi-select is its own feature); tag hierarchies; a tag editor
anywhere but the viewer's info panel; case-insensitive tag matching (tags stay exact, as
the Tag view already is).

## 2. Where per-photo changes live: an overlay table

`item_tags` keeps exactly what the files say. `write_tags` and the scanner do not change.

This is the same decision the tag manager made, for the same reason: `describe()` rewrites
a photo's `item_tags` rows from the file whenever the scanner re-reads it, so a change
folded into that table is lost at the next rescan. A per-photo change is therefore a row in
a second table, applied wherever tags are read.

Two rejected alternatives:

- **Two tables**, one for additions and one for suppressions. The types are more obvious,
  but it is two tables, two indexes and two writers for what is one user decision, and the
  "a tag cannot be both" invariant moves out of a primary key and into code.
- **A `source` column on `item_tags`**, with `write_tags` deleting only `source = 0`. The
  Tag view's existing index would serve both sources unchanged. But suppression still needs
  a third state, and it destroys the invariant that `item_tags` is *what the file says* —
  the property that makes "a rescan rewrites this table" safe to reason about.

## 3. Schema

Migration 7:

```sql
-- The user's per-photo tag changes: a tag added to one photo (added = 1), or one of that
-- photo's own file keywords hidden on it (added = 0). Separate from item_tags for the same
-- reason tag_rules is: a rescan rewrites item_tags from the file, and the user's change
-- must outlive that. Suppressions name the *raw* keyword; additions name it as tag_rules
-- resolves it. The two namespaces meet only in remove_item_tag.
CREATE TABLE item_user_tags (
    item_id INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    tag     TEXT NOT NULL,
    added   INTEGER NOT NULL,
    PRIMARY KEY (item_id, tag)
);
CREATE INDEX item_user_tags_tag ON item_user_tags(tag) WHERE added = 1;
```

`PRAGMA foreign_keys = ON`, so `purge_items` cleans these rows up by itself, as it already
does for `item_tags` and `faces`.

Nothing here can orphan a thumbnail — no `path`, `size`, `mtime_ms` or fingerprint column
changes — so no `bump_thumb_gc_epoch` call, and the tripwire test in `settings.rs` stays as
it is.

### Invariants

- **`item_tags` is the file.** Every row in it came from `describe()`.
- **One row per photo per name.** The primary key. A name that is both a file keyword and a
  user addition cannot be stored twice; the states degenerate correctly (§4).
- **Additions are stored resolved, suppressions raw.** An addition holds the name
  `tag_rules` resolves the typed text to. A suppression holds the keyword as the file
  spells it, because that is the row it suppresses.
- **The rules are still flat.** Nothing here creates a rule, except the removal rule that
  `add_item_tag` deletes.

## 4. Core (`photon-core`)

### Mutations (`library/tags.rs`)

```rust
/// Adds `tag` to one photo, returning the name stored.
pub fn add_item_tag(&self, item_id: i64, tag: &str) -> Result<String>
/// Removes the displayed name `tag` from one photo.
pub fn remove_item_tag(&self, item_id: i64, tag: &str) -> Result<()>
```

`add_item_tag`, in one transaction:

1. Trim; reject empty with `Error::EmptyTagName` (already defined, already surfaced).
2. Resolve through `tag_rules`. A name with a **rename** rule is stored as the rule's
   target: typing `holiday` under `holiday → vacation` stores `vacation`, so the tag the
   user gets is the tag they see.
3. A name with a **removal** rule has that rule deleted — not any rule merged into it, so
   this only ever undoes hiding that exact name, and a tag still reached through a
   separately merged name stays hidden. This is the one global effect a per-photo action
   has; it is reachable only by typing a hidden name exactly, since the suggestions list
   only live names. The
   alternative — refusing the name — was rejected: a tag the user has just typed should not
   be turned away, and storing it literally while a rule hides it would make it vanish from
   the panel on the next read, which reads as a bug.
4. Upsert `(item_id, name, added = 1)`.

`remove_item_tag`, in one transaction:

1. Delete any addition row on that photo whose stored name resolves to the displayed name.
2. Insert `added = 0` for every raw `item_tags` row on that photo whose rule-resolved name
   is the displayed name. A merge means one displayed name can stand for several raw
   keywords, and all of them must go, or the tag reappears.

Because both steps key on `(item_id, tag)`, a name that is both a file keyword and a user
addition ends as a single suppression row, and re-adding it flips that row back to
`added = 1`. The suppression is then lost, which is correct: the result either way is that
the photo carries the tag.

### Readers

`EFFECTIVE_TAGS` becomes a union. Suppressions apply to **raw** keywords, before the rules
resolve names; a `src` column orders file keywords (in file order) ahead of user additions
(in the order added):

```sql
SELECT t.item_id, coalesce(r.target, t.tag) AS tag, t.src, t.seq FROM (
    SELECT it.item_id, it.tag, 0 AS src, it.rowid AS seq FROM item_tags it
     WHERE NOT EXISTS (SELECT 1 FROM item_user_tags u
                       WHERE u.item_id = it.item_id AND u.tag = it.tag AND u.added = 0)
    UNION ALL
    SELECT item_id, tag, 1 AS src, rowid AS seq FROM item_user_tags WHERE added = 1
) t LEFT JOIN tag_rules r ON r.tag = t.tag
WHERE r.tag IS NULL OR r.target IS NOT NULL
```

Every reader that already goes through `EFFECTIVE_TAGS` picks up user tags from this one
edit: `item_tags()` (the panel), `tags_with_counts()` (the sidebar and the tag manager),
and the `group_concat` that builds the search text in `items.rs`. `item_tags()`'s
`ORDER BY seq` becomes `ORDER BY src, seq`.

`TAG_FILTER` keeps its index-probe shape — its comment and
`the_tag_view_is_served_by_its_index` exist to hold it there, and `EFFECTIVE_TAGS`'
`coalesce` is what no index can serve. It gains a third `UNION ALL` arm over
`item_user_tags WHERE tag = ?1 AND added = 1`, served by `item_user_tags_tag`. Its two
existing arms over `item_tags` each gain a per-row
`AND NOT EXISTS (SELECT 1 FROM item_user_tags u WHERE u.item_id = item_tags.item_id
AND u.tag = item_tags.tag AND u.added = 0)`, so a photo whose *other* keyword still answers
to the name stays in the view.

### The three readers that would go blind

`rename_tag`, `hide_tag` and `tag_rules()` each decide whether a tag exists with
`EXISTS (SELECT 1 FROM item_tags WHERE tag = ?)`. A tag that exists only because the user
added it by hand fails that test, so without a change it cannot be renamed, cannot be
removed, and its rule would not be listed in Settings. All three must also consider
`item_user_tags` rows with `added = 1`.

## 5. IPC (`photon-app`)

`Engine::add_item_tag(id, tag)` and `Engine::remove_item_tag(id, tag)` wrap the library
call and then `refresh_grid()`, the shape `set_star` already uses.

That refresh is load-bearing. Tagging a photo changes Tag-view membership *and* what search
matches, since tags feed the search text through `EFFECTIVE_TAGS`. Without travelling the
refresh chain the database moves while the grid shows stale rows.

No `ScanReport` counter is needed: these mutations are not made by a scan, so
`touched_rows` does not gate them.

Three files per command, in order:

1. `commands.rs` — `add_item_tag(engine, id, tag) -> CmdResult<()>` and
   `remove_item_tag(engine, id, tag) -> CmdResult<()>`.
2. `ipc.rs` — two `#[tauri::command(async)]` wrappers that only delegate.
3. `app.rs` — both names in `tauri::generate_handler![…]`.

No Tauri plugin is involved, so `capabilities/default.json` is untouched.

`ui/src/lib/api.ts` gains `addItemTag(id, tag)` and `removeItemTag(id, tag)`.
`ViewerItem.tags` already exists on both sides, so the hand-written mirror needs nothing
else.

## 6. UI

`ui/src/lib/tag-editor.svelte.ts` holds the logic, as `album-membership.svelte.ts` does for
the album checkboxes, and for the same reasons: it is testable under vitest's `node`
environment, and the optimistic update needs care.

`createTagEditor({ add, remove })` holds the bound photo's tags, the set of names with a
call in flight, and the text being typed. `bind(itemId, tags)` is called when the viewer
loads a photo.

- **Optimistic**, like the star toggle and the album checkboxes: a chip that lags its own
  click reads as broken.
- **The revert is guarded by the photo id.** The user can navigate while a call is in
  flight, and a revert landing on the next photo would change a tag nobody touched.
- **A second click on a name already in flight is dropped, not queued.**

Suggestions come from `api.listTags()`, filtered **in TypeScript**, case-insensitively —
matching the rule that case-insensitive matching is never done in SQL here. The list is
fetched when the viewer opens and refreshed on `library_changed`.

`Viewer.svelte` renders the photo's tags as chips, each with an `×`, and one text input that
adds on Enter, placed beside the album checkboxes in the info panel. What is left in the
component is effect wiring, verified by `svelte-check` and a new line on the README's
`## Manual smoke checklist`, not by a component test: vitest runs with
`environment: 'node'` and cannot render a `.svelte` file.

## 7. Testing

Every test below must be demonstrated to fail with its change reverted.

**Core (`library/tags.rs`):**

- A user tag survives the scanner re-reading the file's keywords — the `update_item_meta`
  re-read pattern of `a_rule_survives_rereading_the_keywords`.
- A suppressed file keyword stays suppressed across the same re-read.
- Adding a name under a rename rule stores the target.
- Adding a globally removed name drops the rule, and the tag is live everywhere again.
- Removing a merged display name suppresses every raw keyword behind it.
- A user tag is listed, counted, viewed and searched exactly like a file keyword.
- A tag that exists only as a user tag can be renamed and removed in the tag manager, and
  its rule is listed by `tag_rules()`.
- Adding a name that is already a file keyword on that photo shows it once, not twice.

**Schema (`library/schema.rs`, `library/mod.rs`):** a
`the_seventh_migration_adds_item_user_tags` test seeded from `MIGRATIONS[..6]`, plus the
breakage the bump is meant to cause — the two hardcoded version numbers in `library/mod.rs`
(the opened version and `SchemaTooNew`'s `supported`), the table count, and the table-name
list. These are updated, never loosened to `MIGRATIONS.len()`; the hardcoding is the
tripwire.

**Plan (`library/items.rs`):** `the_tag_view_is_served_by_its_index` extended so the new
union arm is held to an index probe too, rather than drifting into a scan.

**App (`engine.rs`):** tagging a photo bumps the grid version and the Tag view follows —
the shape of `set_star_writes_the_ini_and_the_grid_follows`.

**UI (`tag-editor.svelte.test.ts`):** the optimistic flip, the revert on failure, the photo-id
guard on that revert, and a second click dropped while one is in flight.

**Gates:** `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`,
`npm run check`, `npm test`.
