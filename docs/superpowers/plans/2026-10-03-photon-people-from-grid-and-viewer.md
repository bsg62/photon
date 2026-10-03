# People from the Grid and the Viewer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove a photo from a person, and put a photo's face with a person, from the grid's tile menu and the viewer's context menu, through one "Who is this?" dialog; and make the info panel's names links to the person.

**Architecture:** Three new `Library` operations in `library/people.rs` (`name_faces`, `name_items`, `remove_from_person`) behind three commands, all through `Engine::write_people`. `viewer_item` gains the id of photon's face behind each plate and outline. The UI adds a dialog modelled on the keyword dialog (`createPersonPicker` + `PersonPicker.svelte`, an overlay in `App.svelte`), pure helpers for the toast wording and the viewer's hit test, and menu items in `Grid.svelte` and `Viewer.svelte`.

**Tech Stack:** Rust (photon-core, photon-app), Tauri 2 IPC, Svelte 5 runes + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-10-03-photon-people-from-grid-and-viewer-design.md`. The People design it builds on is `docs/superpowers/specs/2026-10-03-photon-people-design.md` (read its "The operations" and "As built" 3, 8, 11, 14). Read CLAUDE.md's "Recognising people", "IPC is three files per command", "TypeScript mirrors", "A dialog belongs in App.svelte", "What reloads the viewer", "There is no component test harness" and "Conventions" before any task.

## Global Constraints

- **photon never writes `.picasa.ini` names or photo files.** Picasa's faces are never changed; a photo a person is on through a Picasa face linked to them stays theirs.
- **Only confirmed faces carry a name.** `name_faces` confirms; `remove_from_person` rejects (records the rejection, clears `person_id` and `confirmed`).
- **Every People write goes through `Engine::write_people`** (the `people_write` lock, one transaction, `refresh_after_write`, a face-pass request).
- **Names are resolved by the backend's one rule** (`clean`: trimmed, empty refused; `same_name`: Unicode lowercase, in Rust; contacts linked by name through `link_contacts_by_name`). The UI's hint uses `nameChoice` from `ui/src/lib/people.ts`, which mirrors it.
- **IPC:** `commands.rs` → `ipc.rs` → `app.rs`'s `generate_handler!`, the mirror in `ui/src/lib/api.ts` in the same commit, and an answer in `crates/xtask/screenshots/mock.js` (the xtask test fails without it).
- **A dialog belongs in `App.svelte`**, counted in `covered`, handing focus back after `await tick()`.
- **The viewer must not reload for a face change:** `pictureChanged`'s `Pick` leaves `faces` and `unnamedFaces` out; keep it so.
- **Colours are tokens, icons are `Icon` names** from `lib/icons.ts` (`no-literals.test.ts`, `tokens.test.ts`).
- **Every new test fails with its change reverted** (exact replacement from a saved copy, then `touch`); a probe that passes is a finding. Markup and effect wiring with no seam say so in the commit message.
- **Never launch the GUI.** Gates: Rust (`cargo fmt --all`, `--check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`), `npm run check` (0 errors, 0 warnings), `npm test`, `cargo test -p xtask`.
- Commits end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`. Do not push. Branch `feat/people-ui`.

## Decisions made in planning

1. **The switch off needs no check of its own**: with "Find faces" off every `detected_faces` row is gone, so `name_faces` finds no face; it then writes nothing and creates no person (`Ok(None)`), and `remove_from_person` rejects nothing. The spec's "refused with the switch off" is met by that, without a new error.
2. **A person is created only when at least one of the given faces exists**, so a stale request never leaves an empty named person behind.
3. **The viewer's keys must not fire while the dialog is open over it**: the viewer listens on `<svelte:window>`, so the dialog stops the propagation of every `keydown` inside it (typing "H" in the name field would otherwise hide the photo).
4. **The context-menu hit test maps the pointer through the face layer's own bounding rectangle**, which already includes the viewer's zoom and pan, so the mapping is one division and does not duplicate the zoom code.

## Review Focus

1. **A photo already the person's with one other unnamed face** (Anna confirmed, a stranger unnamed): "Add to Anna" from the grid must skip it as already hers, never name the stranger. → Task 2, `name_items_skips_a_photo_already_the_persons`.
2. **Typing in the dialog over the viewer**: no viewer key (H, arrows, Delete, Escape beyond closing the dialog) acts. → Task 4 is markup; Task 3's factory test `the dialog swallows every key it receives` pins the handler the dialog uses; the smoke checklist lists it.
3. **A face's id that went stale** (an edit re-detected the photo between opening the menu and choosing): nothing is written, no empty person is created, the toast says nothing was named. → Task 2, `name_faces_of_faces_that_are_gone_creates_no_person`.
4. **Removing from a person a photo they are on through Picasa**: it stays, and the toast says why, rather than "Removed 1 photo". → Task 2, `remove_from_person_counts_photos_picasa_keeps`; Task 3, the toast wording test.
5. **Right-clicking where two faces overlap**: the menu acts on the face whose box is smallest among those under the pointer (the foreground face is usually inside the larger background rectangle less often than the reverse; smallest is the face the pointer is most specifically on). → Task 3, `faceAt picks the smallest box under the point`.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/photon-core/src/library/faces.rs` | `item_named_detected_faces` returns face ids; a detection reader with ids | 1 |
| `crates/photon-app/src/commands.rs` | `ItemFace.face_id`, `UnnamedFace`, `viewer_item` | 1 |
| `ui/src/lib/api.ts`, `crates/xtask/screenshots/mock.js` | mirrors | 1, 2 |
| `crates/photon-core/src/library/people.rs` | `name_faces`, `name_items`, `remove_from_person`, their types and tests | 2 |
| `crates/photon-app/src/{commands,ipc,app}.rs` | three commands | 2 |
| `ui/src/lib/person-picker.svelte.ts` (new) | the dialog's behaviour | 3 |
| `ui/src/lib/people.ts` | toast wording: `namedItemsMessage`, `removedMessage` | 3 |
| `ui/src/lib/faces.ts` | `toLayer`, `faceAt` | 3 |
| `ui/src/components/PersonPicker.svelte` (new), `App.svelte`, `Grid.svelte`, `lib/library.svelte.ts` | the dialog, the grid's two items | 4 |
| `ui/src/components/Viewer.svelte` | menu items, clickable outlines, linked names | 5 |
| `CLAUDE.md`, `docs/smoke-checklist.md`, `crates/xtask/src/screenshots.rs`, `mock.js` | docs, a screenshot of the dialog | 5 |

---

### Task 1: Which face is which in the viewer

**Files:**
- Modify: `crates/photon-core/src/library/faces.rs` (`item_named_detected_faces`), `crates/photon-core/src/library/detected_faces.rs` (a reader with ids)
- Modify: `crates/photon-app/src/commands.rs` (`ItemFace` construction in `viewer_item`, `ViewerItem.unnamed_faces`)
- Modify: `crates/photon-core/src/library/faces.rs` (`ItemFace` gains `face_id`)
- Modify: `ui/src/lib/api.ts`, `crates/xtask/screenshots/mock.js`, every UI test literal of `ItemFace`/`ViewerItem`
- Test: `commands.rs` tests (the viewer_item tests already build faces — follow them)

**Interfaces:**
- Produces: Rust `ItemFace { key, name, left, top, right, bottom, face_id: Option<i64> }`; `UnnamedFace { left, top, right, bottom, face_id: Option<i64> }` (serde camelCase); `ViewerItem.unnamed_faces: Vec<UnnamedFace>`.
- Produces: TS `ItemFace { key; name; left; top; right; bottom; faceId: number | null }`; `UnnamedFace { left; top; right; bottom; faceId: number | null }`; `ViewerItem.unnamedFaces: UnnamedFace[]`.
- Produces: `Library::item_named_detected_faces(item) -> Result<Vec<(i64 /*face id*/, Rect, i64 /*person*/, String)>>`; `Library::item_detected_faces_with_ids(item) -> Result<Vec<(i64, Rect, Option<i64> /*person*/, bool /*confirmed*/)>>`.

The rules (spec, "Which face is which in the viewer"):
- A plate built from a confirmed detection (`named_detected`): `face_id` = that detection's id.
- A plate of Picasa's (`item_faces`, `key` `p:<id>` when its contact is linked): `face_id` = a detection with `merge::same_face(plate rect, detection rect)` that is confirmed as that same person (`p:<id>`), if any; `None` for a `c:` plate (an unlinked contact — the face beneath may be someone else's, people spec As built 14).
- An unnamed outline that is a detection: its id.
- An unnamed outline that is Picasa's: the detection beneath it (`same_face`) that no named person is confirmed on, if any; that detection is not drawn (Picasa's rectangle is), so without this the outline could not be named.

- [ ] **Step 1: Failing tests** (`commands.rs` tests, beside the existing `viewer_item` face tests; reuse their fixture helpers for writing detections and Picasa faces):
  - `a_detections_plate_carries_its_face_id` — a detection confirmed as a named person: the plate's `face_id` is the detection's id.
  - `a_linked_picasa_plate_carries_the_detection_beneath` — Picasa face of contact C linked to person P, a detection at the same place confirmed as P: the plate's `face_id` is that detection; with the detection confirmed as another person Q instead, `None`.
  - `an_unlinked_contacts_plate_has_no_face_id` — `c:` plate over any detection: `None`.
  - `an_unnamed_outline_carries_its_face_id` — an unnamed detection: its id; an unnamed Picasa face with an unconfirmed detection beneath: the detection's id; with none beneath: `None`.
- [ ] **Step 2: Run** (`cargo test -p photon-app --lib face_id`) — FAIL (no field).
- [ ] **Step 3: Implement.** Return ids from the two readers (keep `item_detected_faces` for its other callers, or change them all — your call, say which). In `viewer_item`, set `face_id` per the rules; `UnnamedFace` replaces `Rect` in `unnamed_faces` (search for every reader of `unnamed_faces`: `search_face_counts` does not use it; the viewer and its tests do). Mapping of rectangles through the edit is unchanged: compare rectangles in the frame `viewer_item` already compares them in (detections are as shown; Picasa's go through `merge::shown` first).
- [ ] **Step 4: Run, PASS. Probes:** drop the person check on a linked plate (the Q case fails); give `c:` plates the detection beneath (the unlinked test fails); leave Picasa outlines `None` (the outline test fails).
- [ ] **Step 5: Mirrors.** `api.ts`: `ItemFace.faceId`, new `UnnamedFace`, `ViewerItem.unnamedFaces: UnnamedFace[]`, doc comments saying what the id is and when it is null. `mock.js`'s `viewerItem` gives its faces `faceId` (some null). Fix UI test literals `npm run check` reports. `ui/src/lib/picture.ts`'s `Pick` must still exclude `faces`/`unnamedFaces` (check, do not change).
- [ ] **Step 6: Gates and commit**

```bash
git commit -m "feat(people): the viewer knows which of photon's faces each plate and outline is"
```

---

### Task 2: Naming and removing faces and photos

**Files:**
- Modify: `crates/photon-core/src/library/people.rs` (three operations, two result types, tests)
- Modify: `crates/photon-app/src/commands.rs`, `ipc.rs`, `app.rs` (three commands)
- Modify: `ui/src/lib/api.ts`, `crates/xtask/screenshots/mock.js`

**Interfaces:**
- Produces (Rust, `library::people`):

```rust
/// A skipped photo, for the toast.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedItem { pub id: i64, pub file_name: String }

/// One kind of skipped photo: the first `LISTED_SKIPPED` of them and how many there were.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped { pub items: Vec<SkippedItem>, pub count: i64 }

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedItems {
    /// The person the photos' faces now belong to; `None` when nothing was named.
    pub person: Option<i64>,
    /// The person's name as stored (an existing person's own spelling).
    pub name: String,
    pub named: i64,
    pub already: Skipped,
    pub several: Skipped,
    pub none: Skipped,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedItems { pub removed: i64, pub kept_by_picasa: i64 }

pub const LISTED_SKIPPED: usize = 20;

impl Library {
    pub fn name_faces(&self, faces: &[i64], name: &str) -> Result<Option<i64>>;
    pub fn name_items(&self, items: &[i64], name: &str) -> Result<NamedItems>;
    pub fn remove_from_person(&self, person: i64, items: &[i64]) -> Result<RemovedItems>;
}
```

- Produces (commands): `name_faces(engine, faces: Vec<i64>, name: String) -> CmdResult<Option<i64>>`, `name_items(engine, items: Vec<i64>, name: String) -> CmdResult<NamedItems>`, `remove_from_person(engine, person: i64, items: Vec<i64>) -> CmdResult<RemovedItems>`, each through `engine.write_people("…", |lib| …)`.
- Produces (TS): `SkippedItem { id; fileName }`, `Skipped { items; count }`, `NamedItems { person: number | null; name; named; already; several; none }`, `RemovedItems { removed; keptByPicasa }`; `api.nameFaces(faces, name)`, `api.nameItems(items, name)`, `api.removeFromPerson(person, items)`.

Semantics (spec "Operations"; Decisions 1-2):
- **`name_faces`**: `clean(name)?`; in one transaction: keep only the faces whose rows exist; none → `Ok(None)`, nothing written. Resolve the person: `person_named(&tx, &name, -1)` (no person is excluded); else a new row `INSERT INTO people (name) VALUES (?1)` (the table's defaults give `ignored = 0`); then each face: `UPDATE detected_faces SET person_id = ?2, confirmed = 1, ignored = 0 WHERE id = ?1` and `DELETE FROM face_rejections WHERE face_id = ?1 AND person_id = ?2`; finally `link_contacts_by_name(&tx)`. Returns the person.
- **`name_items`**: `clean(name)?`; one transaction; resolve the person *without creating* first (`person_named`); for each photo (deduplicated, in the order given): **already** if the resolved person exists and has a confirmed face on it, or a Picasa face on it whose contact is linked to them (the Person view's own condition: `items.rs`'s `p:` filter — `faces f JOIN person_contacts pc ON pc.contact = f.contact WHERE pc.person_id = ? AND f.item_id = ?`); otherwise its candidates are `SELECT id FROM detected_faces WHERE item_id = ?1 AND ignored = 0 AND (person_id IS NULL OR confirmed = 0 OR person_id IN (SELECT id FROM people WHERE name IS NULL))` — not confirmed as a named person, not ignored; **none** if zero, **several** if more than one, else the one face is collected. If any face was collected: create the person if needed, and name the collected faces exactly as `name_faces` does (share one private function taking `&Transaction`, used by both). File names from `items.path`'s last component for the skipped lists, up to `LISTED_SKIPPED` each, `count` the full number. `name` in the answer is the stored name (an existing person's own spelling, or the cleaned name).
- **`remove_from_person`**: one transaction; `is_named(&tx, person)?` else `Err(NotAPerson(person))`; for the photos: reject every face of that person on them (`confirmed` either way) with `reject_faces`' own statements (factor them into a private function on `&Transaction` used by both); then `removed` = photos that had such a face and are now in neither arm of the Person view's condition, `kept_by_picasa` = photos still matched by the Picasa arm.

- [ ] **Step 1: Failing tests** (`people.rs` tests; use the module's `library` helper and its way of writing Picasa faces and contacts — `the_offer_counts_only_named_contacts` or similar tests show how):
  - `name_faces_with_a_new_name_makes_a_person` — returns `Some(p)`, `p` named "Ben", the face confirmed under `p`.
  - `name_faces_joins_an_existing_person_whatever_the_case` — "anna" joins Anna; no second person.
  - `name_faces_links_a_picasa_contact_of_that_name`.
  - `name_faces_clears_the_faces_rejection_from_that_person` — a face rejected from Anna, then named Anna: confirmed under Anna, no rejection row left.
  - `name_faces_names_a_face_too_small_for_a_vector` — a face with no embedding: confirmed, and the Person view (`entries_for(GridView::Person, "p:<id>")` or however the module's Person-view tests read it) lists the photo.
  - `name_faces_un_ignores_the_face`.
  - `name_faces_of_faces_that_are_gone_creates_no_person` — ids that do not exist: `Ok(None)`, `named_people_count` unchanged.
  - `name_items_names_a_photos_only_unnamed_face`.
  - `name_items_skips_photos_with_several_unnamed_faces_and_lists_them` — two such photos: `several.count == 2`, both file names listed, neither named.
  - `name_items_skips_a_photo_with_no_unnamed_face` — a photo with no detection: `none`.
  - `name_items_skips_a_photo_already_the_persons` — a photo with Anna confirmed and one unnamed face: `already`, the unnamed face untouched. And a photo where Picasa names a contact linked to Anna plus one unnamed detection: `already` too.
  - `name_items_does_not_count_an_ignored_face` — one unnamed face plus one ignored: the unnamed one is named.
  - `name_items_lists_at_most_twenty_of_a_kind` — 21 several-face photos: 20 listed, `count` 21.
  - `remove_from_person_rejects_the_persons_faces_on_the_photos` — Anna's confirmed and suggested faces on the photos: both rejected (rows in `face_rejections`, `person_id` NULL); Ben's face on the same photo untouched; `removed` counts the photos.
  - `remove_from_person_counts_photos_picasa_keeps` — a photo Anna is on through a linked Picasa face and a detection: the detection rejected, `kept_by_picasa == 1`, `removed == 0`.
  - `remove_from_person_refuses_an_unnamed_group` — `NotAPerson`.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement** per the semantics above. Doc comments carry the reasons: why "already" exists (naming the other face would put a stranger under the name), why ignored faces are not candidates, why no person is created for gone faces.
- [ ] **Step 4: Run, PASS. Probes:** drop the "already" check (its test fails); drop `ignored = 0` from candidates (the ignored test fails); drop the rejection delete in `name_faces` (its test fails); create the person before checking the faces exist (the gone test fails); remove the Picasa arm from the kept count (the Picasa test fails).
- [ ] **Step 5: Commands, IPC, mirrors, mock.** Three commands through `write_people` (labels "naming faces", "naming photos", "removing photos from a person"); `ipc.rs` wrappers taking `faces: Vec<i64>`/`items: Vec<i64>` and `name: String`; `app.rs` entries; `api.ts` types and functions; `mock.js` answers (`name_faces: () => 3`, `name_items` returning a `NamedItems` with one `several` entry, `remove_from_person` returning `{ removed: 1, keptByPicasa: 0 }`). A command test in `commands.rs` that `remove_from_person` on an unnamed group is `kind: "notAPerson"`.
- [ ] **Step 6: Gates and commit**

```bash
git commit -m "feat(people): name faces and photos, and take photos from a person, by id"
```

---

### Task 3: The dialog's behaviour, the toast wording, the hit test

**Files:**
- Create: `ui/src/lib/person-picker.svelte.ts`, `ui/src/lib/person-picker.svelte.test.ts`
- Modify: `ui/src/lib/people.ts`, `ui/src/lib/people.test.ts` (toast wording)
- Modify: `ui/src/lib/faces.ts`, `ui/src/lib/faces.test.ts` (`toLayer`, `faceAt`)

**Interfaces:**
- Consumes: `nameChoice`, `NameChoice` (`people.ts`); `NamedItems`, `RemovedItems`, `Person` (`api.ts`, Task 2); `Box`, `faceBox` (`faces.ts`).
- Produces (`person-picker.svelte.ts`):

```ts
export type PickerTarget = { kind: 'face'; face: number } | { kind: 'items'; items: number[] };

export interface PersonPickerDeps {
  nameFaces(faces: number[], name: string): Promise<number | null>;
  nameItems(items: number[], name: string): Promise<NamedItems>;
}

export function createPersonPicker(deps: PersonPickerDeps): {
  readonly visible: boolean;
  readonly busy: boolean;
  readonly target: PickerTarget | null;
  draft: string;
  /** "Who is this?" for a face, "Add 3 photos to…" for photos. */
  readonly title: string;
  show(target: PickerTarget): void;
  close(): void;
  /** Named people narrowed by the draft, best match first (exact, starts-with, contains),
   *  compared as `nameChoice` compares. */
  suggestions(people: readonly { id: number; name: string }[]): { id: number; name: string }[];
  /** "Add to Anna" / "New person “Ben”" / '' — from `nameChoice`, so the hint and the
   *  backend agree. */
  hint(people: readonly { id: number; name: string | null }[]): string;
  /** Writes; closes before the write (as the keyword dialog does); returns the toast line,
   *  or null when there was nothing to do (blank draft, busy). */
  submit(name?: string): Promise<string | null>;
  /** The dialog's keydown handler: Escape closes (returns true so the caller hands focus
   *  back), Enter submits the draft; every key's propagation is stopped, since the viewer
   *  listens for single-letter keys on the window. */
  keydown(e: Pick<KeyboardEvent, 'key' | 'stopPropagation' | 'preventDefault'>): 'close' | 'submit' | null;
};
export type PersonPicker = ReturnType<typeof createPersonPicker>;
```

- Produces (`people.ts`): `namedItemsMessage(r: NamedItems): string`, `namedFaceMessage(name: string): string` ("This is Anna."), `removedMessage(name: string, r: RemovedItems): string`.
- Produces (`faces.ts`): `toLayer(clientX: number, clientY: number, rect: { left: number; top: number; width: number; height: number }, frameW: number, frameH: number): { x: number; y: number }`; `faceAt(x: number, y: number, boxes: readonly Box[]): number` (the index, or −1).

- [ ] **Step 1: Failing tests.**
  - `person-picker.svelte.test.ts` (node project, server runtime — the factory uses `$state` and getters only; follow `tag-picker.svelte.test.ts`): `show captures the target and clears the draft`; `the title says what it acts on` ("Who is this?", "Add 1 photo to…", "Add 3 photos to…"); `suggestions narrow and rank like the keyword dialog` (exact first, then starts-with, then contains; case-insensitive, Unicode: "émile" finds "Émile"); `the hint says whether Enter adds or creates` ("Add to Anna" for "anna"; "New person “Ben”"; "" for blank); `submit closes before the write and names the face` (deps.nameFaces called with `[face]` and the trimmed name; `visible` false before the promise resolves; the returned line is "This is Anna."); `submit for photos returns the grid's toast line` (uses `namedItemsMessage`); `a blank or busy submit does nothing`; `the dialog swallows every key it receives` (a fake event: `stopPropagation` called for "h", "ArrowLeft", "Escape", "Enter"; Escape → 'close', Enter → 'submit').
  - `people.test.ts`: `namedItemsMessage` for: all named ("Added 5 photos to Anna."); one photo ("Added 1 photo to Anna."); several skipped with names ("… 2 have more than one unnamed face: IMG_1.jpg, IMG_2.jpg — open them to choose the face."); more skipped than listed ("… 23 have more than one unnamed face: a.jpg, b.jpg, … and 3 more — open them to choose the face."); already ("1 is already Anna's."); none ("1 has no face photon found."); nothing named at all ("Nothing was added to Anna." followed by the reasons). `removedMessage`: "Removed 3 photos from Anna."; with kept ("Removed 1 photo from Anna. 2 stay: Picasa names Anna on them."); all kept ("2 stay with Anna: Picasa names Anna on them.").
  - `faces.test.ts`: `toLayer` maps a point through a scaled, offset rect (rect 100,50,800×600 for a 400×300 frame: client (500, 350) → (200, 150)); `faceAt picks the smallest box under the point`; −1 when none.
- [ ] **Step 2: Run** — FAIL.
- [ ] **Step 3: Implement.** The toast functions: plural rules as `counted` in `tag-picker.svelte.ts`; list `items` file names joined with ", ", and "and N more" when `count > items.length`. `faceAt`: among boxes containing the point (inclusive edges), the smallest area; ties → the first. `toLayer`: `x = (clientX - rect.left) * frameW / rect.width` (and y alike); a zero-sized rect gives `{ x: -1, y: -1 }`.
- [ ] **Step 4: Run, PASS. Probes:** `keydown` without `stopPropagation` for letters (its test fails); `faceAt` returning the first box (the smallest test fails — make sure the test's larger box comes first); `namedItemsMessage` without the "and N more" (its test fails).
- [ ] **Step 5: Gates and commit**

```bash
git commit -m "feat(people): the person dialog's behaviour, the toasts' wording, the viewer's hit test"
```

---

### Task 4: The dialog, and the grid's menu

**Files:**
- Create: `ui/src/components/PersonPicker.svelte`
- Modify: `ui/src/App.svelte` (the overlay, `covered`, focus hand-back, an `openPersonPicker(target, returnTo)` the grid and viewer call)
- Modify: `ui/src/components/Grid.svelte` (two menu items)
- Modify: `ui/src/lib/library.svelte.ts` (`removeFromPerson(person, ids)`, refetching the collections like `removeFromAlbum`)

**Interfaces:**
- Consumes: `createPersonPicker`, `PersonPicker`, `PickerTarget` (Task 3); `removedMessage` (Task 3); `api.nameFaces`, `api.nameItems`, `api.removeFromPerson` (Task 2); `library.people` (named people are the `p:` entries of `library.people`: `Person { key, name, count }`).
- Produces: `App.svelte` passes `onnameperson: (ids: number[]) => void` to `Grid` and (Task 5) `onnameface: (faceId: number) => void` to `Viewer`.

- [ ] **Step 1: `PersonPicker.svelte`** — copy `TagPicker.svelte`'s structure (backdrop, `role="dialog"`, `aria-modal`, `.focus-container`, the field focused on open, `dismiss()` and `submit()` handing focus back through `onclosed`), with: the title from `picker.title`; the field bound to `picker.draft`; the hint (`picker.hint(named)`) under it; the suggestion list (`picker.suggestions(named)`, each a button submitting that name); Cancel and a primary "Name" button. `named` = `library.people` filtered to keys starting with `p:` mapped to `{ id: Number(key.slice(2)), name }`. The dialog's `onkeydown` calls `picker.keydown(e)` and acts on its answer (Decision 3). Its `z-index` is above the viewer's (`.viewer` is 20; the keyword dialog's backdrop is 30 — use the same). Tokens only.
- [ ] **Step 2: `App.svelte`** — `const personPicker = createPersonPicker({ nameFaces: api.nameFaces, nameItems: api.nameItems })`; render `<PersonPicker picker={personPicker} onclosed={closePersonPicker} />` beside `TagPicker`; `covered` gains `personPicker.visible`; `openPersonPicker(target, from: 'grid' | 'viewer')` remembers where focus returns; `closePersonPicker` hands focus back after `await tick()` — to the grid (`grid?.focus()`) or to the viewer (the viewer exports or already has a way to focus its root; if not, add `export function focus()` to `Viewer.svelte` focusing its `.focus-container` root). The toast line from `submit` goes to `library.notify`, errors to `library.reportError` (as `TagPicker.svelte` does).
- [ ] **Step 3: `Grid.svelte`'s tile menu** — after the keyword items:
  - "Add {subject} to a person…" → `withSelection(async (ids) => onnameperson(ids))`.
  - While `library.info.view === 'person'` and `library.info.person` starts with `p:`: "Remove {subject} from “{library.personName(library.info.person)}”" → `withSelection((ids) => library.removeFromPerson(Number(person.slice(2)), ids))`; `library.removeFromPerson` awaits the command, refetches the collections, and notifies `removedMessage(name, result)`.
- [ ] **Step 4: Check** — `npm run check` 0/0, `npm test`, `cargo test -p xtask`. No test seam for the markup; say so in the commit.
- [ ] **Step 5: Commit**

```bash
git commit -m "feat(people): the person dialog, and adding or removing photos from the grid"
```

---

### Task 5: The viewer, and the documentation

**Files:**
- Modify: `ui/src/components/Viewer.svelte`, `ui/src/App.svelte` (the viewer's new callbacks)
- Modify: `CLAUDE.md`, `docs/smoke-checklist.md`, `crates/xtask/src/screenshots.rs`, `crates/xtask/screenshots/mock.js`
- Modify: `docs/superpowers/specs/2026-10-03-photon-people-from-grid-and-viewer-design.md` (an "As built" section only if anything departed)

**Interfaces:**
- Consumes: `ItemFace.faceId`, `UnnamedFace.faceId` (Task 1); `api.removeFromPerson`, `api` reject (`api.rejectFaces`) (Task 2 / existing); `toLayer`, `faceAt`, `faceBox`, `containedBox` (Task 3); `openPersonPicker` via an `onnameface(faceId)` prop and `onperson(key)` prop from `App.svelte`.

- [ ] **Step 1: The context menu.** On `contextmenu`, compute every face's box as drawn — named and unnamed, independent of whether the info panel is open (a derived beside `faceBoxes` that is not gated on `info`; still empty while cropping) — map the pointer with `toLayer` through the face layer element's `getBoundingClientRect()` (Decision 4), and `faceAt`. Remember the hit in the menu state. Items, before the existing ones, separated:
  - Hit an unnamed face with a `faceId`: "Name this face…" → close the menu, `onnameface(faceId)`.
  - Hit a plate with a `faceId`: "Not {name}" → `api.rejectFaces([faceId])`.
  - No hit: "Not {name}" for each plate with a `faceId` (deduplicated by person key; rejecting all that person's faces with ids on this photo).
  - A face without an id offers nothing.
  After a reject, notify "{name} taken off this photo." (a line in `people.ts` with a test is fine, or inline — your call, say which); the refresh chain re-reads the viewer item; in that person's view the photo leaves the view and the viewer's `orphaned` handling keeps it on screen, as after Hide.
- [ ] **Step 2: Clickable outlines.** With the info panel open, an unnamed outline with a `faceId` is a `<button class="face">` (`aria-label="Name this face"`, `title` the same) calling `onnameface(faceId)`; others stay `div`s. Check the click does not start a pan (stop the pointerdown from reaching the pan handler) and that the outline's focus ring follows the project's rules.
- [ ] **Step 3: Linked names.** Each person chip in the info panel's People list is an `info-link` button → `onperson(face.key)`; App's handler closes the viewer, calls `searchBox.cancel()`, `mainPage.showGrid()`, `library.setPersonView(key)`, then focuses the grid after `await tick()` (as `searchFrom` does). Deduplicate chips by key if the same person appears twice on a photo.
- [ ] **Step 4: Check** — `npm run check` 0/0, `npm test`.
- [ ] **Step 5: Screenshot.** A `SHOTS` entry `person-picker-dark` (`theme=dark&do=personpicker`) and a mock action that opens the tile menu on a tile and clicks "Add photo to a person…" (follow the `keyword` action); update the screenshot count words in CLAUDE.md (two places: "thirty-one" → "thirty-two") and any test that states it. Run `cargo run -p xtask -- screenshots --only person-picker-dark` (Chromium is at `/usr/bin/chromium`) and look at it; fix what is wrong.
- [ ] **Step 6: Docs.** CLAUDE.md, in "Recognising people": the three operations (by id, through `write_people`; `name_items`' already/several/none rule and why; Picasa's faces never changed), the viewer's `faceId` rules (which detection a plate or outline is), the dialog over the viewer stopping key propagation (Decision 3). Smoke checklist, a "People from the grid and the viewer" section: add from the grid (one face, several faces listed in the toast, a photo already the person's skipped), remove in a person's view (a Picasa-only photo stays, the toast says so), right-click a face in the viewer (Name this face…, Not Anna), click an outline with the info panel open, type "h" and arrows in the dialog over the viewer (nothing happens behind it), click a name in the info panel (the Person view opens).
- [ ] **Step 7: Gates and commit**

```bash
git commit -m "feat(people): name and take off faces in the viewer; names in the info panel open the person"
```

---

## Self-review

- **Spec coverage:** remove from the grid (Task 4) and the viewer (Task 5); add from the grid (Tasks 2, 4) and the viewer (Tasks 1, 5); one dialog (Tasks 3-4); info panel links (Task 5); Picasa unchanged and reported (Tasks 2, 3); skip-and-list (Tasks 2, 3); `faceId` rules (Task 1); toast wording (Task 3); hit test (Task 3); testing list (each task); not-in-design items untouched.
- **Placeholders:** fixture construction in Tasks 1-2 points at the existing tests to copy; every behaviour has its test named and its expected value stated.
- **Types:** `NamedItems`/`RemovedItems`/`SkippedItem`/`Skipped` (Task 2) are what Task 3's messages take; `PickerTarget` (Task 3) is what Task 4's App passes; `faceId` (Task 1) is what Task 5 reads; `onnameperson`/`onnameface`/`onperson` are defined in Tasks 4-5.
- **Review Focus:** five lines, each with its owning task's test or the smoke checklist.
