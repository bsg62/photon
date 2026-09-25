# Captions

2026-09-25. Approved in conversation the same day, section by section.

## What it is

photon shows the caption a photo carries in its own metadata - the one Picasa writes when you
type under a photo, and Lightroom, Bridge and digiKam write as a description - under the photo
in the viewer and in the slideshow, and in the info panel, and finds it with search. photon
only reads it: a caption is part of the photo file, and photon never writes a photo.

"Caption" below is the photo's text. The viewer already has a *file-name line* whose class and
module are called `caption` (`Viewer.svelte`'s `.caption` button, `ui/src/lib/caption.ts`); the
new UI element and module are named `photo-caption` to keep the two apart.

## Decisions

- **Stored on the item row** (`items.caption`), read by the scanner, backfilled through
  `EXIF_VERSION`. Reading it from the file on each viewer open was rejected because search
  could then not find a caption without opening every file; a side table was rejected because a
  photo has at most one caption.
- **Sources, in order:** XMP `dc:description`, then IPTC 2:120 (Caption-Abstract). XMP wins
  when both exist and differ: it is the newer standard, and a tool that edits only one of them
  edits XMP. **EXIF `ImageDescription` is never read**: cameras fill it with their own name
  ("OLYMPUS DIGITAL CAMERA") or blanks, and trusting it would caption thousands of photos with
  a camera model. Picasa writes its captions to IPTC, so nothing real is lost.
- **Shown under the photo** in the viewer and during the slideshow, and in the info panel; not
  on grid tiles, so the grid index does not grow. The user chose this over the info panel alone
  and over tile tooltips.
- **Read-only.** No caption editing.

## Reading (`photon-core`)

- `keywords.rs` becomes the embedded-text reader: `read_embedded(path) -> Embedded { keywords:
  Vec<String>, caption: Option<String> }`, over the same single bounded read of
  `xmp::MAX_PREFIX` bytes, so a photo is still opened once. `read_keywords` remains as a wrapper
  while anything calls it.
- `xmp::description_from_xml(xml) -> Option<String>`: the `dc:description` → `rdf:Alt` →
  `rdf:li` whose `xml:lang` is `x-default`, otherwise the first `rdf:li`. Same `quick_xml` event
  walk as `subjects_from_xml` - text split across events joined, character references resolved.
  An `rdf:li` outside `dc:description` is not a caption.
- `iptc::caption_in(prefix) -> Option<String>`: the first dataset 2:120 found by the existing
  APP13 walker, decoded by the existing `decode` (UTF-8, else Latin-1).
- The chosen text is trimmed; empty or whitespace-only is `None`. Internal line breaks are kept
  in storage. It is capped at 2,000 characters, cut on a character boundary, so a stray huge
  field cannot bloat every search row.

## Storage (migration 18)

```sql
ALTER TABLE items ADD COLUMN caption TEXT;
```

NULL until a scan reads the photo. `NewItem` gains `caption: Option<String>`; every writer of an
item row writes it - `insert_items`, `update_items` and `update_item_meta` (the backfill's
writer). `Library::item_caption(id)` reads it for the viewer; `Item` does not carry it, since
only the viewer needs it. The literal version numbers in `library/mod.rs` and `schema.rs` move
to 18; the migration test seeds from `MIGRATIONS[..17]`.

**Backfill.** `metadata::EXIF_VERSION` 2 → 3, so the first scan after the upgrade re-describes
every unchanged photo once and writes its caption through `update_item_meta`. Its doc comment
records what generation 3 added. The cost is one bounded read per photo on that scan, as the
last bump cost.

## Search

`search_entries` selects `i.caption`; a non-empty caption is one more haystack beside file and
folder name, camera, keywords and date, matched case-insensitively in Rust like the rest. No
`caption:` prefix: prefixes confine a term and nobody has asked to confine one to captions.

## IPC and how a change reaches the screen

`ViewerItem` gains `caption: Option<String>`, mirrored in `api.ts` as `caption: string | null` in
the same commit, and every `ViewerItem` literal in the UI tests follows. No new command.
`GridEntry` is untouched.

A caption changes only with the file (`changed`) or through the backfill (`enriched`), both
already in `touched_rows`, so the existing refresh chain carries it. The viewer re-reads
`viewer_item` on every grid version and swaps in the fresh item, so a caption arriving while
the photo is open appears without a reload. `pictureChanged` must not compare the caption: a
caption-only change must never blank the photo.

## UI

- **Under the photo** (`Viewer.svelte`): `<p class="photo-caption">`, a sibling of `.bar`, not
  inside it - the slideshow's quiet state fades `.bar` (`.quiet .bar { opacity: 0 }`) and the
  caption must stay. Centred above the bar, on the same translucent chrome surface the bar uses
  (never directly on the photo), with the bar's existing tokens; no new colour. Clamped to two
  lines with an ellipsis, full text in `title`. Hidden while the crop tool is open. Absent when
  the photo has no caption.
- **Info panel:** a **Caption** section first, above People, full text with line breaks kept
  (`white-space: pre-line`), shown only when there is a caption.
- **`ui/src/lib/photo-caption.ts`**, pure: `photoCaptionLine(caption: string | null): string |
  null` - trims, collapses runs of whitespace and line breaks to single spaces, `null` for
  empty. Tested with vitest; the component renders what it returns.
- **Screenshots:** `mock.js`'s canned `viewer_item` gains a caption, so the viewer and
  viewer-info shots show both places.
- **README:** a "Captions" paragraph in "Camera data, keywords, people and albums": read from
  XMP and IPTC, EXIF's camera text ignored, searchable, not edited by photon.

## Testing

Every test is shown to fail with its own change reverted, on an input where the reverted code
answers differently.

- **XMP:** `x-default` chosen over an earlier other-language entry; the first entry when none is
  `x-default`; a split-text and a character-reference caption; an `rdf:li` in `dc:subject` or
  elsewhere is not a caption; no `dc:description` is `None`.
- **IPTC:** 2:120 read beside 2:25 (the existing fixture already carries one); Latin-1 decoded;
  first of two 2:120 records wins; none is `None`.
- **Embedded:** XMP wins over IPTC when both differ; IPTC alone is used; whitespace-only is
  `None`; the 2,000-character cap cuts on a character boundary (a multi-byte character straddling
  the cut); keywords unchanged by the refactor.
- **Library:** migration 17 → 18 leaves existing rows with `caption` NULL; `insert_items`,
  `update_items` and `update_item_meta` each store the caption (three tests, each probed by
  dropping its column from that writer's SQL).
- **Scanner:** a new photo's caption is stored; an unchanged photo whose `exif_version` is 2 is
  re-described and gains its caption, counted as `enriched`, and `touched_rows` is true.
- **Search:** a caption word finds the photo; the same library without the caption haystack
  finds nothing for it.
- **App:** `viewer_item` reports the caption.
- **UI:** `photoCaptionLine` rules. `pictureChanged` returns false for two items differing only
  in caption (a pin: the function never compared it, and must not start).

## Smoke checklist additions

1. A photo captioned in Picasa shows the caption under the photo, in the info panel, and during
   a slideshow after the controls fade.
2. Searching a word of the caption finds the photo.
3. A photo whose camera wrote only EXIF `ImageDescription` shows no caption.

## Limits

- A caption only in EXIF `ImageDescription` is not shown (by design, above).
- A caption Picasa kept only in its own database, never in the file, does not appear. Picasa 3
  writes captions into the photo, so this matters only for photos it could not write (read-only
  files, formats it does not write IPTC into).
- XMP in a sidecar `.xmp` file is not read; neither are keywords today.

## Not in this feature

Editing captions (it would write the photo), a `caption:` search prefix, captions on grid
tiles, and EXIF `ImageDescription` with a junk filter.
