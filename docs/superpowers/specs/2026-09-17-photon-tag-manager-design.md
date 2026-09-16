# photon — Tag Manager Design

**Date:** 2026-09-17
**Status:** Approved design
**Builds on:** v0.13.0

## 1. What changes

Tags are the keywords photon reads from each photo (XMP `dc:subject`, IPTC 2:25) into
`item_tags`. There is no way to tidy them: a misspelt or duplicate keyword shows in the
sidebar forever.

This design adds a **Tags** section to the Settings dialog that lists every tag with its
photo count and lets the user **rename** or **remove** one. Renaming onto an existing tag
**merges** the two. Every change is listed in the same section and can be **restored**.

### The promise is unchanged

photon never writes photo files, and keywords stay read only. A rename or removal is a
**rule held in `library.db`** that photon applies when it reads tags. The files keep their
keywords; other applications see them unchanged; removing the rule brings the original
back. The Settings copy says so.

### Out of scope

Writing keywords back to files; adding a tag to a photo; editing tags per photo; case-
insensitive tag matching (tags stay exact, as the Tag view already is); rules on faces or
albums.

## 2. Where rules apply: on read

`item_tags` keeps exactly what the files say, and `write_tags` and the scanner do not
change. A rule is applied by every reader of `item_tags`.

The rejected alternative applied rules in `write_tags`. Restoring a rule would then need
the original keyword, so `item_tags` would carry a second `raw` column; every rule change
would rewrite rows across the library; and a merge collides with the
`(item_id, tag)` primary key when one photo carries both names. Two copies of the truth,
kept in step by hand, is the drift this codebase keeps avoiding.

Applying on read means a rule change is one row plus a grid rebuild, a photo indexed later
picks up the rules with no extra code, and restoring is a `DELETE`.

## 3. Schema

Migration 6:

```sql
-- A keyword the user renamed (target set) or hid (target NULL). Keywords are read from the
-- photo and never written, so the rule is applied when tags are read, not in item_tags.
CREATE TABLE tag_rules (
    tag    TEXT PRIMARY KEY,
    target TEXT
);
```

`library/mod.rs`'s literal version (5 → 6) and table count (9 → 10) are updated, not
loosened.

### Invariants

Kept by the `Library` methods below, inside one write transaction each:

1. **Flat.** No rule's `target` is itself a ruled `tag`. A tag's effective name is found in
   one lookup, never a chain.
2. **No identity rules.** No row has `target = tag`.
3. **No empty targets.** A `target` is never empty or all whitespace.

The effective name of a keyword `k` is: `k` if it has no rule; `target` if its rule has one;
nothing (hidden) if its rule's `target` is `NULL`.

## 4. Core (`photon-core`)

### Mutations

- **`rename_tag(from, to)`.** Refuses a blank `to` (after trimming; `to` is stored trimmed).
  No-op when `from == to`. Otherwise, in one transaction:
  1. every rule with `target = from` gets `target = to` (keeps invariant 1: the tags
     already merged into `from` follow it);
  2. upsert `from → to`;
  3. if `to` itself has a rule, the rename re-points at a ruled name. `to` is the name the
     user typed and sees, so its own rule is deleted: `to` becomes a live name again;
  4. delete every row with `target = tag` (invariant 2 — this is what makes renaming back
     to the original name a restore).
- **`hide_tag(tag)`.** Upserts `tag → NULL` and sets `target = NULL` on every rule whose
  target is `tag`: removing a merged tag removes everything merged into it.
- **`restore_tag_rule(tag)`.** Deletes that one rule. A tag merged into a name that was
  later hidden had its rule set to `NULL` by `hide_tag`, so it is restored on its own,
  under its original name.
- **`tag_rules()`** returns every rule, sorted case-insensitively in Rust by `(tag
  lowercased, tag)`.

### Readers

One helper, `EFFECTIVE_TAGS`, a SQL fragment selecting `(item_id, tag)` with rules applied:

```sql
SELECT t.item_id, coalesce(r.target, t.tag) AS tag
FROM item_tags t LEFT JOIN tag_rules r ON r.tag = t.tag
WHERE r.tag IS NULL OR r.target IS NOT NULL
```

Every reader goes through it or through `tag_filter` below. A reader of `item_tags` that
bypasses both shows hidden and renamed keywords, so the doc comment on `item_tags` in
`schema.rs` names them.

- **`tags_with_counts`** groups `EFFECTIVE_TAGS` by tag and counts `DISTINCT item_id`, so a
  photo carrying both halves of a merge counts once.
- **`item_tags(id)`** (the viewer) returns effective tags for one item, de-duplicated,
  keeping first-seen order.
- **Search**'s correlated `group_concat` reads effective tags, so a hidden keyword no
  longer matches and a renamed one matches by its new name. The old name no longer matches:
  the user renamed it.
- **The Tag view** filters with `tag_filter`:

  ```sql
  AND i.id IN (
      SELECT item_id FROM item_tags
      WHERE tag IN (SELECT tag FROM tag_rules WHERE target = ?1)
      UNION ALL
      SELECT item_id FROM item_tags
      WHERE tag = ?1 AND NOT EXISTS (SELECT 1 FROM tag_rules WHERE tag = ?1)
  )
  ```

  `UNION ALL` rather than `OR`, which SQLite may answer with a scan; both arms are
  equality probes on `item_tags_tag`. A plan test,
  `the_tag_view_is_served_by_its_index`, pins that, so a rewrite through the
  `coalesce` join (which cannot use the index) fails rather than silently scanning every
  keyword. `tag_rules` gets `CREATE INDEX tag_rules_target ON tag_rules(target)` in the same
  migration for the first arm.

## 5. IPC (`photon-app`)

Four commands, each through `commands.rs`, `ipc.rs` and `app.rs`:

| command | returns |
|---|---|
| `list_tag_rules` | `Vec<TagRule>` |
| `rename_tag(from, to)` | `()` |
| `hide_tag(tag)` | `()` |
| `restore_tag_rule(tag)` | `()` |

`TagRule { tag: String, target: Option<String> }`, mirrored in `api.ts` as
`{ tag: string; target: string | null }` in the same commit.

The three mutations call a new `Engine::tags_changed()`, the counterpart of
`albums_changed()`: it rebuilds the grid through `refresh_grid()`, which is what reaches
the Tag view, search results and the sidebar (which re-reads `listTags` on
`library_changed`). No `ScanReport` counter is involved; this is not a scan.

**The Tag view follows a rename.** If the view is `Tag` with argument `from`, `rename_tag`
re-enters `Tag` with argument `to` before rebuilding, so the grid keeps showing the same
photos. Hiding the viewed tag leaves the argument alone; the grid empties, as deleting the
viewed album does.

## 6. UI

`SettingsSection` gains `'tags'`, listed between Folders and About.

The section has two parts:

- **Tags.** A filter box (case-insensitive substring, in TS), then one row per tag from
  `listTags`: name, photo count, **Rename** and **Remove**.
  - Rename turns the name into an input. Enter commits, Escape or blur cancels. A blank
    name is refused inline. A name that already exists in the list asks
    "Merge "holiday" into "vacation"?" before committing.
  - Remove asks "Remove "X" from photon? The keyword stays in your photo files, and you can
    restore it below."
- **Changes.** One row per `listTagRules` entry: `holiday → vacation` or `DSC_import —
  removed`, with **Restore**. Hidden when there are no rules.

Both lists reload on `library.info.version`, with the same `stale` guard the Folders counts
use. Errors go to `library.reportError`.

The logic lives in `ui/src/lib/tags.ts`: `renameCheck(from, to, existing)` returning
`'blank' | 'same' | 'merge' | 'ok'`, `filterTags(tags, query)`, and `ruleLabel(rule)`, each
with vitest tests. The component is wiring.

## 7. Testing

Rust, in `library` (each shown failing with its change reverted):

- `a_renamed_tag_is_listed_viewed_searched_and_shown_by_its_new_name`
- `merging_two_tags_counts_each_photo_once`
- `renaming_a_merged_tag_moves_everything_merged_into_it` (flatness)
- `renaming_a_tag_back_to_its_original_name_removes_the_rule`
- `renaming_onto_a_renamed_name_revives_it`
- `hiding_a_merged_tag_hides_everything_merged_into_it`
- `restoring_a_rule_brings_the_original_tag_back`
- `a_blank_rename_is_refused`
- `a_rule_survives_a_rescan_that_rereads_keywords`
- `the_tag_view_is_served_by_its_index`

In `photon-app`: `renaming_the_viewed_tag_keeps_the_view_on_it`.

UI: `renameCheck`, `filterTags`, `ruleLabel`.

README smoke checklist: rename, merge, remove and restore a tag from Settings; the sidebar
and an open Tag view follow.
