# The library: schema, thumbnails, edits, duplicates, faces and people

Part of photon's architecture notes, split out of `CLAUDE.md` (which indexes them). Read the section for an area before changing it: each rule here is a bug that was found once.

### Schema

`library/schema.rs` holds `MIGRATIONS: &[&str]`, one entry per version, each run in its own
transaction with `PRAGMA user_version` bumped after it. A library from a newer photon is refused
with `SchemaTooNew`. SQLite runs in WAL mode, so `library.db` has `-wal`/`-shm` siblings.

A schema bump breaks tests on purpose: `library/mod.rs` asserts the literal version number
twice (the opened version and `SchemaTooNew`'s `supported`) and the table count once, and the
migration tests seed from `MIGRATIONS[..N-1]`. Update the numbers rather than loosening them
to `MIGRATIONS.len()`; the hardcoding is the tripwire. An index that serves a
specific query gets a plan test (`the_recent_view_is_served_by_its_index`), so drift between the
index and the `ORDER BY` fails rather than silently regressing. A query that reads *every* live
photo writes `+missing_since IS NULL`: photon never runs `ANALYZE`, and without statistics the
bare term makes SQLite walk a partial index on that predicate - `items_size`, in random table
order, 7-12x slower at 300k photos. `library/mod.rs` has the reasoning; each such query has a
plan test, and a new one needs its own.

The same missing statistics make **partial indexes tie**: without `ANALYZE` SQLite costs every
partial index the same, a constant reduction whatever its predicate, and on a tie takes the
one created *last*. A view's grid query is safe by shape: its `folder_id` group key and
equality pick the `(folder_id, taken_at)` index holding its rows. A count is not: a scan with
no `folder_id` term ties with every partial index whose predicate its WHERE implies, and wins
only by creation order. Today `starred_count`, `video_count` and the Hidden count beat
`items_pending`, `items_size` and `items_recent` that way, so a later partial index whose
*predicate* those counts' WHERE implies - a new `WHERE missing_since IS NULL` index, say -
takes them over; `the_starred_and_video_counts_are_served_by_their_indexes` (and Hidden's plan
test) is the tripwire. The counts are cached (`counts_epoch`), so a flip costs a cache refill,
not every grid version. The same tie is why the All view has no `items_visible`
(`missing_since IS NULL AND hidden = 0`, same shape as the view indexes, schema 22): every
visible view's WHERE implies it, and created after `items_starred` and `items_videos` it took
Starred and Videos over, the whole library read for a grid of 3%.

**Reads are pooled, writes are one connection.** `Library::reader()` returns a `Result` and never
waits on another reader: it hands out an idle pooled connection or opens one (at most eight are
kept). `writer()` is a single mutexed connection.

**The thumbnail workers' decodes share a byte budget** (`thumbs/service.rs`: `DECODE_BUDGET`,
1 GiB, by `decode_cost`'s estimate from the stored size and format; `Budget`, in turn by
ticket, one decode always let in). Eight workers on a folder of 100 MP scans held several
gigabytes. Taken before `decode_lock` and before the in-flight marker: a photo waiting for
room holds neither (`a_photo_waiting_for_room_holds_no_marker`), and inside the lock a
suspect's bounded wait would have waited out the decodes of photos queued for room too.

**Thumbnail garbage collection is gated.** Any write that can orphan a thumbnail — deleting an
item, or changing anything the thumbnail key is made of (`path`/`size`/`mtime_ms`, the
fingerprint columns, and `edit_turns`/`edit_crop`) — must call
`settings::bump_thumb_gc_epoch` inside its own transaction. Today that is `purge_items`,
`purge_at` (the scanner's purge by path), `update_items`, `remove_watched_folder`,
`set_item_edit` and `move_items` (a moved row's old key is garbage). The tripwire test in
`settings.rs` enumerates those six, so a *new* orphaning write is not caught
automatically; `apply_moves` returns before calling `move_items` for a batch with no move, since
the epoch moves even for no rows and every import would make the walk of the cache due
(`an_import_that_follows_no_move_orphans_no_thumbnail`); the seven-day `THUMB_GC_MAX_AGE`
in `engine.rs` bounds the damage of a miss.

**Edits are rendered by the backend, and the thumbnail key includes them.** An edit
(`photon_core::edit`: EXIF orientation, then quarter turns, then a crop of the turned picture)
lives in `items.edit_turns`/`edit_crop` and is applied in exactly three places: the thumbnail
renderer, the full-size render (`render_picture`: the viewer's picture, a copy for the
clipboard, an export), and the face rectangles in `viewer_item`. The window draws no edit; it
writes one and the refresh chain brings the picture under its new `thumb_key`. **What is
made smaller than the photo is decoded smaller** (`edit::render_fitted`,
`decode::render_decode`): a copy, an export at a size and a cropped photo's preview take a
JPEG through libjpeg-turbo's n/8 decode, at the scale that leaves what the edit keeps at
least the fitted size, and are resized to exactly the size the whole decode fits to (the
crop rounds to other pixels at another scale) - a 48 MP copy peaked at 203 MB, now 60.
Anything that names a thumbnail must use `Item::thumb_key()` (or `Edit::thumb_key` over the
fingerprint, as `map_grid_row` and `live_fingerprints` do); a bare `fingerprint` names the
*unedited* photo's thumbnail. The untouched photo's key is the bare fingerprint on purpose, so
caches from before edits existed stay valid. `set_thumb_state_if_unchanged` compares the edit
too, or a worker that rendered the pre-edit picture marks the row `Ready` with nothing cached
under the new key. For an edited photo `ViewerItem` reports `width`/`height`/`orientation` and
`faces` *as shown*. Every write of an edit takes `Engine.edit_write`, because a turn reads
the edit it builds on. Full-size renders run one at a time (`RENDERING`, in `photon-engine`'s
`engine.rs`: the viewer's picture, a copy and an export take it - held across the render and
never across the write), outside the thumbnail pool that otherwise bounds decode memory.

**The duplicate finder hashes after the scan, in the engine, in two passes.** `items.content_hash`
(XXH3-128, NULL for almost every row) is filled by `photon_core::duplicates::hash_candidates`,
which reads only files sharing a byte size with another live file - this finds byte-identical
copies. `items.percep_hash` (a 64-bit difference hash, `photon_core::similar`) and
`items.similar_group` (a union-find id) find look-alikes - the same picture after a resize or
a re-save - and are filled by `photon_core::similar::update`. **The hash only nominates a
pair; the pixels decide it.** At the hash's 9x8 resolution a second shot of the same pose *is*
the same picture, so `group` unites a pair within the distance only when `same_picture`
agrees - both cached grid thumbnails reduced to 32x32 greyscale, mean removed, mean absolute
difference at most `SAME_PICTURE_MAX_DIFFERENCE` (measured: copies at or under 2.3, same-pose
second shots 19 and up). The answer for each pair of thumbnail keys is kept in the engine's
`hashing` lock between passes (`Verdicts`) - not the reductions, a kilobyte a photo for the
session, which live for one regroup - and is let go of once either key is no hashed photo's,
not when a pass did not ask it (`group` visits buckets in a `HashMap`'s order, so the pair
that joins two photos differs from pass to pass); a pair with an unreadable thumbnail keeps
no answer. Confirming honours `cancel`: a cancelled regroup writes no groups. The same lock
holds a digest of the last finished regroup's input (distance, and every `(id, hash, thumb_key)`)
and of the groups it stored; a pass whose input and stored groups both still match skips the
regroup, which is what keeps a no-op scan from banding the whole library. Candidates come
from four 16-bit bands, each bucket also compared with those one bit away, which is exact up
to `EXACT_RECALL_DISTANCE` (7) - Conservative. Treating the hash as the verdict brings the
false pairs back. Both passes run inside
`Engine::hash_after_scan` (once `hash_duplicates`), requested through `request_similar_pass`
at the end of every `run_scan` but one that
finds its root still offline (the 30-second poll of an unplugged drive, or the startup scan of
one), which read and changed nothing. The pass runs on its own thread, after the scan has
released its slot: inline, it held back `startup`'s `start_watcher` for the session's first
whole-library regroup. Every pass is a thread spawned by `spawn_pass` and counted in
`background_passes` (the face pass's too, below), which is what `shutdown` (`stop_passes`,
bounded) and a test's `Fixture::settle` (`wait_for_passes`, unbounded) wait on - a test that
reads hashes or groups after a scan waits with `settle`, not `wait_for_scans`. They also run, through
`request_similar_pass`, whenever the thumbnail queue has stayed quiet for `THUMB_HASH_SETTLE`
after making new thumbnails ready (`start_thumb_hashing`, `ThumbQueue::wait_drained`), since a
scan's own pass runs while the queue it fed is still rendering and its new photos otherwise
waited for the next scan. Not inside the scanner, so neither of `walk_tree`'s callers can be forgotten, and because a duplicate or a
look-alike is a fact about the whole library, not about one changed file. The perceptual hash
is taken from the photo's **already-cached grid thumbnail**, not from the source file: the
thumbnail renderer is skipped whenever a thumbnail is already cached, so a hash computed inside
the renderer would never run for a single photo in an existing library, only for ones rendered
after the feature shipped. Reading the cache instead means an upgraded library fills in for
every photo whose thumbnail already exists, and a photo is never decoded a second time just to
be hashed. Any write for a file whose *content* changed (its size or mtime) must set
`content_hash = NULL` (today `update_items`, which also clears `percep_hash` and
`similar_group` - a rewritten file has lost whatever picture those described); a row that keeps
a stale hash is never a candidate again. A write for a file that only *moved* is the opposite
case: `move_items` changes the path, and with it the fingerprint and the thumbnail key, and
keeps all three on purpose, the bytes and the picture being the same.
`set_content_hash` refuses a row whose size or mtime moved since the candidate was listed.
`set_item_edit` clears `percep_hash` and `similar_group` too, for the same reason it clears the
thumbnail: a look-alike is a fact about the photo *as shown*, and an edit changes what that is.

**Face detection is a third pass, off until the user switches it on.**
`Engine::request_face_pass` runs `photon_core::face_detect::pass::run` on a thread from
`spawn_pass`, so it is counted in `background_passes` with the look-alike pass and `shutdown`
and `settle` cover it with no second mechanism. It is requested in five places: at the end of
`run_scan`, on the thumbnail queue's drain, by the switch itself, once by `startup` after
the first grid is built, and after every correction of the People data (`write_people`,
below); with the switch off the request is a no-op. That is not everywhere the
look-alike pass is requested: `write_edit`, `remove_folder` and `set_similar_distance` request
that one and no face pass - an edit reaches the face pass through the drain, once the edited
photo's thumbnail has been remade. The drain is the trigger that matters for a new photo, which
is a candidate only once its preview exists; the scan's is the only one a running photon gets
for a library whose thumbnails are all cached; and `startup`'s is what resumes an unfinished
pass on a launch where every root is offline, whose scans request nothing. A pass asks each
step for one row of work before it loads a model (`face_work`, in `run_face_pass`, which names
the first step that has some; only detection, which makes faces, has the pass ask the next
question again): with none it sends its last event and leaves - a probe per step (a photo to
detect and a face to embed, each a walk of its whole table when there is none; a face to
group, an index search over the faces with no group), the last event's count, and no model
load, which is what keeps the request after every scan cheap (a library holding a photo whose
preview can never be read never gets this path, that photo being always a candidate) - and a
detection or embedding step with work reports `running` at once, the next report being a
whole batch away. A pass the quit ends skips its
rebuild and its last count. The
pass reads each photo's **cached 1600 px preview**, never the source, for the reason the perceptual hash reads the grid thumbnail: inside the
renderer it would never run for a photo whose thumbnail is already cached. So its candidate
list has no `online` term, unlike the look-alike pass's: an unplugged drive's photos are
detected from photon's own cache. **The detector (YuNet, through `tract`) runs twice per
picture**, at a 1280 input (`INPUT`) and at 320 (`CLOSE_UP_INPUT`), and the two runs' faces go
through one overlap suppression, in fractions of the picture: 1280 is what finds a group
photo's small faces, and it does not find a face that fills the frame (measured: one taller
than about two thirds of the preview's long side, a head shot or a selfie), which 320 does. A face both runs find comes out once, as the stronger of the two boxes. A face with
a number that is not finite is dropped before the suppression (`Raw::is_finite`): SQLite binds
NaN as NULL, the table refuses it, and the refused batch would be listed first by every pass
after. `items.face_version` records which `DETECTOR_VERSION` looked,
faces found or not; bump the constant when the model file, either input size, or the threshold
or overlap limit in `decode.rs` changes, and every photo is detected again. The pass pages by id: a photo
whose preview cannot be read, or whose decode panics, is skipped *unwritten*, so asked for from the start it would be
handed back for ever. Such a photo stays a candidate - right for a removed cache file, which
comes back, but a file libwebp refuses is never re-rendered, so it costs one failed read per
pass and the progress count stops short of the total. A detection that errors or panics
(`tract` unwinds, unlike rav1d) is written as looked-at with no faces instead, unless every
photo detected in its batch failed and there were at least `BREAKER_FLOOR` (8) of them: that is
taken to be the detector failing, not the photos (the rule assumes it; a count cannot tell), so the
batch is not written and `pass::run` errs (fewer than 8 failing photos are still marked, so a lone
bad photo is not retried on every pass; each later trigger fails one batch again, by design; and
the reverse cost: 8 or more genuinely bad photos with no success in one batch trip it on every
pass, and no photo with a higher id is ever detected - improbable, but it follows from the rule). `write_face_batch`
has two guards, each with a test that fails without it: a row whose size, mtime or edit moved
since it was listed is skipped, and nothing is written with the setting off, read inside the
batch's own transaction so the switch's delete and a batch in flight cannot interleave.
`update_items` and `set_item_edit` clear a photo's detections and its version, beside the hashes
they already clear. The candidate list and `face_progress` each read every live photo, so each
writes `+missing_since IS NULL` and has its own plan test. A pass that only detected rebuilds
through `refresh_grid_derived`: a detection is read by a face search's grid and by the viewer,
never by a collection count. Once at its end if it wrote anything, and during detection at most
every 30 seconds and only while the view is a search whose query reads faces
(`view_reads_faces`) - a branch no test reaches, its only trigger being that wait. A pass that
grouped a face rebuilds through `refresh_grid` instead (below).

**The switch is two values and one lock.** The stored `face_detection` setting is the authority;
`Engine.face_enabled` mirrors it so the pass's cancel check is not a database read per photo.
`set_face_detection` moves the mirror first (a pass in flight stops at its next photo), then
the stored value, and off deletes every detection, every person with their contact links and
rejections (`people`, `person_contacts`, `face_rejections`), and nulls every `face_version`, in
that same transaction. All of it, the mirror's restore on a failed write included, is under
`Engine.face_write`, as an edit is under `edit_write`: two toggles close together otherwise
reached the library's writer in either order and left the two disagreeing - mirror on and
stored off runs the detector over the whole library on every scan and drain while every batch
is refused. The lock is released before the pass request, the rebuild and the event. A pass
that ends with the switch off sends the cleared progress event itself, although the command
has sent one: the batch the switch interrupted can still report "running" after it, and the UI
would be left with a progress line for a pass that is gone.

**Recognising people is the face pass's second and third steps** (spec
`2026-10-03-photon-people-design.md`). After detection, `embed_step` runs
`photon_core::face_embed::pass::run`: a photo with a face whose `embedding_version` is not
`EMBEDDER_VERSION` has its cached preview decoded once, and each such face is aligned by its
five landmarks and turned into 128 numbers by SFace, through `tract` like the detector. A step
of its own rather than part of detection, so a library detected by 0.47.0 is recognised without
being detected again, at the cost of a second decode of a photo with faces. Its candidate query
(`EMBED_CANDIDATES_SQL`) applies the photo's conditions (live, preview ready) *inside* the
page's `LIMIT`, faces driving through a `CROSS JOIN`: outside it, a page made of missing photos
came back empty, and the pass, which reads an empty page as no work, stopped there. A face
narrower than `MIN_FACE_PX` (35) in the decoded preview is marked looked-at with no vector and
is never grouped (measured on LFW faces shrunk into a 1600 px picture: the same person still
matched 91% of the time at 35 px and 58% at 18 px, at a similarity of 0.45). `write_embeddings`
has `write_face_batch`'s two guards. The embedding breaker has detection's floor but counts
*faces*, not photos, and that has a cost: one group photo with 8 or more faces that all fail
(landmarks the aligner refuses), in a batch where no other face is asked about, trips it alone,
on every pass, and no photo with a higher id is embedded.

The **grouping step** (`group_ungrouped_faces`) places every face with a vector, no group and
not ignored (`UNGROUPED`, one string for the step and for `has_ungrouped_faces`; the vector one
the embedder made for that face, `embedding_version` set, of the right length - a malformed
blob matched there kept the probe true for ever), in face order on one thread, by
`people::choose`: the most alike group whose average is at least
`GROUP_SIMILARITY` (0.50), else a new unnamed group (measured on 2,674 LFW faces: 27-40
misplaced at 0.45, 6-11 at 0.50, 2 at 0.55 but with about 35 more of 500 people split - and a
split is one merge to fix, a mixed group a face at a time). **The averages are computed from the
faces at the start of each run, never stored**: a stored one has to be kept right by every
writer that moves a face - each operation, the edit and file-change clearing in `items.rs`, the
switch, the carry-over below - and one forgotten writer leaves it silently wrong. A named
person's average is their confirmed faces alone (`counts_toward_centroid`), so suggestions
cannot pull them towards themselves. Groups left with no face are deleted at the start of a run,
unless named or linked to a contact. Grouping runs during the embed step, and at the pass's end
while any face is still ungrouped (what an operation leaves). During the step it is paced by
`GroupPacer`: the first batch that writes is grouped at once, and each later run waits
`GROUP_COST_FACTOR` (9) times the last run's own duration, so grouping is at most about a tenth
of the step - every run reads every grouped face's vector, and run after every batch a first
recognition would read about 40 GB at 100k photos. Only the timing is kept between runs, never
the groups: an edit or a rewritten file deletes detections without `people_write`, so groups
held over could name deleted faces. The choosing runs inside the writer's transaction, so
it is kept cheap: `people::Group` keeps its sum's length beside the sum (private fields, so
nothing changes one without the other) and `face_embed::dot` sums in eight lanes - about 9 ns
a comparison against 95 ns recomputing both lengths. **Grouping is announced as a data change;
detection is not.** Not because the sidebar's People list, the Person view, `person:` or the
viewer move: they read confirmed faces only, and grouping writes none - only suggestions, new
unnamed groups and the removal of emptied ones. What reads those is `people_page`, which the
People page and its sidebar count of groups to name refetch on `data_changed`, so a pass that
grouped a face rebuilds through `refresh_grid` (`data_dirty`, `counts_epoch`) - at its end,
and during the embed step at most every 30 seconds whatever the view.

**The People page reads `people_page`, not the grid index**: it has no rows, so it does not
travel the refresh chain as a view (the window's side: "The People page is not a view of the
grid", below). `people_page` lists the 200 largest unnamed groups (`LISTED_GROUPS`) with
`unnamed_count` counting all of them; the sidebar's "N to name" is `people_to_name`, equal to
`unnamed_count`, and is read with the collections on `data_changed`. A strip of faces is
fetched a page of at most `MAX_FACE_PAGE` (200) at a time. A write refused with `notAPerson`
(its group is gone) is reported as "That group changed". Each read of the page reads every
visible face, 148 ms at 150,000 (`people_150k_faces` in the grid bench). A face's crop is cut
by `ThumbCache::face_crop` from the cached preview only - never a render, so a page of crops
cannot start a burst of renders - and `ThumbCache` keeps the last eight decoded previews by
key (`decoded_preview`), so a group photo's faces decode it once: a key names one picture,
so a kept decode is never stale. They are let go of when the page closes (`forget_decoded`),
and again after each cut that was on its way then: 46-61 MB held for the session otherwise.

**`people_write` serialises the corrections with grouping.** Every operation on the People data
(`name_group`, `rename_person`, `confirm_faces`, `reject_faces`, `merge_people`,
`set_person_ignored`, `set_faces_ignored`, `delete_person`, and the grid's and the viewer's
`name_faces`, `name_items` and `remove_from_person`) runs through `Engine::write_people`,
which holds the lock for the write, then rebuilds through `refresh_after_write` and requests a
face pass; each grouping run takes the same lock. An operation never places a face itself: a
rejected face, or one no longer ignored, is left ungrouped, and the pass's grouping step places
it, passing over every group it was rejected from (`face_rejections`). A merge (`merge_into`,
behind `merge_people` and a name another person has) leaves out, ungrouped, every face of the
merged group that was rejected from the person it merges into: grouping puts a face taken out
of Anna in another group, and naming that group Anna would otherwise make it a confirmed Anna.

**The grid and the viewer name and take off faces by id** (spec
`2026-10-03-photon-people-from-grid-and-viewer-design.md`). `name_faces` names faces as a person
resolved by the one rule (`clean`, `same_name`, contacts linked by name): confirmed, no longer
ignored, any rejection from that person forgotten; it makes a person only when one of the faces
still exists, so a stale request leaves no empty named person behind, and it names a face too
small for a vector, which puts the photo in the Person view without ever shaping suggestions.
`name_items` is the grid's, by photo, and **never confirms a face the user did not choose**: a
photo's candidates are its detections not confirmed as a named person, not ignored, not in an
ignored group and not under a face Picasa names (linked or not, through the photo's edit:
`merge::shown`, then `merge::same_face` - that face is someone already, drawn as Picasa's
plate), and the photo is named only with exactly one. It is skipped as *already* when the person
has a confirmed face on it, or Picasa names on it a contact linked to them or an unlinked contact
of that name - the one this very write would link (`link_contacts_by_name`), so it counts whether
or not the person exists yet - checked first, because the photo's one *other* face is then a
stranger the user did not mean; as *rejected* when a candidate was rejected from the person (a
guess per photo never overrules "Not Anna"; `name_faces`, where the user picks the face, does
clear the rejection); as *several* with more than one candidate (listed by file name, so the user
opens it and picks the face in the viewer; "the largest face" was asked about and declined); and
as *none* with no candidate ("no unnamed face": its faces may all be someone's or ignored). An
ignored face is no candidate, so a background stranger the user put away does not make a photo
ambiguous. `on_person_view`, the "already" test, keeps hidden photos on purpose: in the Hidden
view "Add to Anna" on a hidden photo that shows Anna must not name its other face.
`remove_from_person` is "Not this person" for every face of the person on each photo, confirmed
or suggested, through `reject_in`; a photo the person is on through a linked Picasa face stays
in their view, and `kept_by_picasa` counts it so the toast can say why. The person dialog lists
`named_people` (every named person, read when it opens), not the sidebar's `people_with_counts`,
which leaves out a person with no visible photo whom a typed name still joins. **None of the three
changes Picasa's faces**: photon never writes a name to an INI. **Which detection a viewer face
is** is `ItemFace.face_id`/`UnnamedFace.face_id` (`viewer_item`): a detection's plate or outline
carries its own id; Picasa's plate of a *linked* person carries the detection beneath it
(`merge::same_face`) confirmed as that same person; an unnamed Picasa outline the detection
beneath it that no named person is confirmed on (it is drawn in that detection's place); and a
plate of an unlinked contact (`c:`) carries none, since the face beneath may be someone else's.
A face with no id is Picasa's alone, and the viewer offers nothing on it: its context menu
hit-tests every face as drawn (`viewer_faces.rs`, below) whether or not the info panel shows
the outlines, and offers "Name this face…" on an unnamed face, "Not Anna" on her plate, and
elsewhere "Not …" once for each person photon has a face of on the photo. **The viewer's "Not
Anna" is photo-level**, the grid's Remove for one photo (`remove_from_person`), so it is
offered only where photon holds a face of hers, and its message (`removed_message`) says when
Picasa keeps her there. A rejection reloads nothing (`picture_changed` leaves the faces out);
in that person's view the photo leaves the grid and `orphaned` keeps it on screen, as after
Hide.

**Only confirmed faces carry a name.** A face the rule puts with a named person is a suggestion
(`confirmed = 0`), listed among `people_page`'s suggestions and nowhere else: the Person view,
the People list's counts, `person:` and the viewer's name plates read confirmed faces only.
Naming an unnamed group confirms its faces, and so does merging one into a person; renaming a
named person does not confirm their suggestions, which the user has not looked at. A name
another person has, compared without case in Rust, merges into them. **A person's key** is
`p:<id>`, or `c:<hash>` for a Picasa contact no person is linked to: the Person view's argument,
`GridInfo.person`, `Person.key` and `ItemFace.key`, prefixed so nothing depends on what a hash
may contain; an argument with neither prefix gives an empty grid. A contact is linked to the
person whose name it carries (`link_contacts_by_name`) when `upsert_contacts` records it and
when a group is named or renamed - the user typing a name Picasa uses is saying it is the same
person - and a linked contact is that person everywhere: their key and name in the viewer, their
Person view, and `person:` finds the contact's Picasa-only photos by the person's name. The
viewer draws a face once: a confirmed detection over a named Picasa face, linked or not, leaves
the plate to Picasa's, and one over an unnamed Picasa face takes its outline's place - so a
detection confirmed as Ben over a Picasa face named for a contact Anna that no person is linked
to shows only Anna's plate in the viewer, while Ben's Person view lists the photo. The Picasa
name offered for an unnamed group counts only its faces that sit on a Picasa face under a named
contact (more than half of those must be one contact's): an unnamed Picasa face at the same
place neither votes nor hides the named one. `people.id` and `detected_faces.id` are
`AUTOINCREMENT` - migration 25 rebuilds `detected_faces`, released in 0.47.0, to get it -
because the UI holds those ids (`people_page`) and acts on them later: SQLite otherwise hands
the highest deleted id to the next insert, and a confirm or a rename would land on a face or
person the user never saw. Every named person is in `people_page`'s People section, with a
`face_count` of 0 when none of their confirmed faces is visible: off it they could not be
renamed, merged or deleted, yet `face_data_summary` would still count them.

**What clears what.** An edit or a rewritten file deletes the photo's detections, and with them
their group, confirmation and rejections: re-detected and grouped again, a confirmed face comes
back as a suggestion (carrying it across a turn or a crop would mean mapping rectangles through
the edit, for a case one click fixes). A detector re-run over an unchanged picture - a
`DETECTOR_VERSION` bump - must not do that to a whole library: `write_face_batch` hands each new
face the group, confirmation, ignored flag and rejections of an old face at the same place
(`merge::same_face`), each old face to one new face, best overlap first (`pairs_by_fit`: the
centre test alone let a small face inside a large one take the large face's name). The old
vector goes too, with no `embedding_version`: the face is embedded again, and meanwhile counts
towards its group's average but is not placed by it (`UNGROUPED` asks for the version) -
without it a re-run emptied every named person's average for the whole embedding step. A
renamed or moved file or folder keeps its row (`move_items`, not `update_items`: the content is
the same, so the detections, their groups, confirmations and rejections stay, and a face's
rectangle is in fractions of the picture as shown, which a move does not change), so it keeps
them as it keeps its albums; it loses them only where it is still a purge and a new row - out of
the watched folders and back, a copy whose original goes later, a move between volumes the
watcher scanned half-way (above). A named person whose confirmed faces are all gone
has no average, so draws no suggestions until a group is named for them again. **Hidden
photos' faces are grouped** like any other, so unhiding is instant, and appear in no People
reader: the page's two face queries, the People list, the Person view and the search grid each
filter `hidden = 0` and `missing_since IS NULL` (`hidden_photos_are_in_no_person_reader`,
`a_hidden_photo_is_in_no_section`). They still count towards their group's average.

**Two tables of faces, in two frames.** Picasa's `faces` rows are fractions of the *unedited*
picture and are mapped through the edit on read; `detected_faces` rows are fractions of the
picture *as shown*, because that is the preview the detector reads. `face_detect::merge` is the
one place they meet: `merge::shown` is the single mapping of a Picasa rectangle into the
picture as shown (the viewer's named faces go through it too), and its rule for "the faces on
this photo" is every Picasa face the edit still shows plus each detection that is none of them,
where either rectangle's centre inside the other is the same face - not an overlap ratio, since
Picasa draws a face loose and YuNet tight. `viewer_item` and `search_face_counts` both go
through it, so the viewer and `has:face`/`faces:N` cannot disagree, and a new reader of either
table goes through it too. A Picasa face no INI names counts as a face under that rule, with
detection on or off: left out, it would hide the detection lying over it.

There is no `COLLATE NOCASE` anywhere and `lower()` is ASCII-only without SQLite's ICU
extension (C, which this project does not take), so **case-insensitive matching is done in
Rust**, not in SQL.

