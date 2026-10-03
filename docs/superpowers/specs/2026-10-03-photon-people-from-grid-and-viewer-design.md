# People from the grid and the viewer

2026-10-03. Designed in conversation the same day, after 0.48.0 shipped the People page
(`2026-10-03-photon-people-design.md`). The user, having used it: "Works very well, but we
should improve the UI a bit" — remove a photo from a person, and associate a photo's face with a
person, from the grid and the viewer, not only from the People page.

## What the user gets

- **Remove from a person.** In a person's view, the grid's tile menu says "Remove 3 photos from
  “Anna”". In the viewer, the context menu says "Not Anna" for each person named on the photo.
  Either is the People page's "Not this person": the face leaves Anna and is never suggested for
  her again.
- **Add to a person.** The grid's tile menu says "Add 3 photos to a person…"; the viewer's says
  "Name this face…" when right-clicking a face photon found that has no name, and with the info
  panel open an unnamed outline can be clicked. Both open one "Who is this?" dialog, which lists
  the people as you type and makes a new person from a new name.
- **Names in the info panel are links** to that person's view, as the camera and lens are links
  to a search.

## Decisions

- **Removing is rejecting.** The same operation as "Not this person": the rejection is recorded
  and the next grouping run places the face elsewhere. Removing a photo from a person rejects
  every face of that person on it, confirmed or suggested: "this photo is not Anna".
- **Picasa's faces are not changed.** photon never writes `.picasa.ini`. A photo in Anna's view
  because Picasa named her there stays in it; the grid says how many stayed and why. The viewer
  offers "Not Anna" only on a face photon found.
- **Naming from the grid is by photo, never a guess.** A photo is named only when it has exactly
  one face that could be meant (below). Photos with more are skipped and listed by file name in
  the toast, so the user opens them and chooses the face in the viewer. Asked and answered:
  skip and list, not "the largest face".
- **One dialog for both places**, modelled on the keyword dialog (`createTagPicker`,
  `TagPicker.svelte`): an overlay in `App.svelte`, its logic a tested `.svelte.ts` factory.

## The backend

### Which face is which in the viewer

`ItemFace` (a name plate) and the unnamed outlines gain the id of photon's face that *is* that
face, when there is one:

- `ItemFace.face_id: Option<i64>` — for a plate of a detection confirmed as a person, its own id;
  for a plate of Picasa's under a contact linked to a person, the detection beneath it
  (`merge::same_face`) that is confirmed as that same person, if any; otherwise `None`. A plate
  of an unlinked contact has `None` (As built 14 of the people spec: the face beneath may be
  someone else's).
- `ViewerItem.unnamed_faces` becomes `Vec<UnnamedFace { rect, face_id: Option<i64> }>`: for an
  outline of a detection, its id; for an unnamed Picasa face, the detection beneath it that no
  named person is confirmed on, if any (that detection is not drawn — the Picasa rectangle is —
  so without this the outline could not be named).

`pictureChanged`'s `Pick` already leaves `faces` and `unnamedFaces` out; it stays that way.

### Operations

Each through `Engine::write_people` (the `people_write` lock, `refresh_after_write`, a face-pass
request), in one transaction, refused with the switch off.

- **`name_faces(faces: &[i64], name) -> i64`** — the person the faces now belong to. The name is
  resolved as naming a group resolves it (`clean`, `same_name` against named people, contacts
  linked by name); a name nobody has makes a new person (a new `people` row with that name). The
  faces get `person_id` = that person, `confirmed = 1`, `ignored = 0`, and any rejection of that
  face from that person is deleted (the user has now said it is them). A face too small to have
  a vector can be named: it puts the photo in the person's view and `person:`, and never shapes
  suggestions, having nothing to count. A face whose row is gone is skipped. A group left with
  no face is deleted by the next grouping run, as today.
- **`name_items(items: &[i64], name) -> NamedItems`** — the grid's: for each photo, its
  candidate faces are the detections that are not confirmed as a named person and not ignored.
  A photo is skipped as **already** when the person (as resolved) has a confirmed face on it or
  Picasa names a contact linked to them on it; as **none** when it has no candidate; as
  **several** when it has more than one; otherwise its one candidate is named, through
  `name_faces`' own path in the same transaction. Returns the person, how many photos were named,
  and the skipped photos of each kind (`id` and file name, at most 20 listed per kind, plus the
  count). Ignored faces are not candidates: a background stranger the user ignored does not make
  a photo of one person ambiguous.
- **`remove_from_person(person, items: &[i64]) -> RemovedItems`** — rejects every face of
  `person` (any `confirmed`) on those photos, through `reject_faces`' own path. Returns how many
  photos left the person's view and how many stayed because Picasa names a contact linked to
  that person on them. Refused unless `person` is a named person (`NotAPerson`).

Hidden and missing photos: the grid and the viewer never offer them in a view that does not show
them, and the operations act on what they are given.

### Commands

`name_faces`, `name_items`, `remove_from_person`, through the three IPC files, `api.ts` and
`mock.js`.

## The UI

- **The dialog** (`createPersonPicker`, `PersonPicker.svelte`, in `App.svelte` beside
  `TagPicker`, counted in `covered`, handing focus back after `await tick()`). Shows what it acts
  on ("Who is this?" for one face; "Add 3 photos to…" for photos), a field, the named people
  narrowed as you type (by `nameChoice`'s rule, so the list and the hint agree), and the hint
  "Add to Anna" / "New person “Ben”". Enter or a click commits; Escape closes. The targets are
  captured at `show()`, as the keyword dialog captures its selection.
- **The grid's tile menu**: "Add {subject} to a person…" always; "Remove {subject} from “Anna”"
  while the view is a named person's (`p:`); none for an unlinked contact's view (`c:`).
- **The toast** after naming from the grid: "Added 5 photos to Anna." plus, per kind skipped,
  "2 have more than one unnamed face: IMG_0141.jpg, IMG_0153.jpg — open them to choose the
  face.", "1 is already Anna's.", "1 has no face photon found." After removing: "Removed 3
  photos from Anna." plus "2 stay: Picasa names Anna on them." The wording is a pure, tested
  function.
- **The viewer's context menu**: right-click hit-tests the face rectangles as drawn (the same
  geometry as the outlines, `faceBox`). On an unnamed face with a `faceId`: "Name this face…". On
  a plate with a `faceId`: "Not Anna". Elsewhere on the photo: "Not …" for each plate with a
  `faceId`. A face without one offers nothing (Picasa's alone).
- **Clickable outlines**: with the info panel open, an unnamed outline with a `faceId` is a
  button opening the dialog for that face.
- **Info panel names**: each person chip is a link to `library.setPersonView(key)`, closing the
  viewer first, as the camera and lens links do.
- In a person's view, a photo the user removed leaves the view; the viewer stays on it and the
  arrow keys carry on, as after hiding.

## Testing

Every new test is shown to fail with its change reverted.

- `name_faces`: a new name makes a person; an existing name in another case joins them; a
  Picasa contact's name links it; a rejection of that face from that person is cleared; a
  too-small face can be named and appears in the Person view; an ignored face is un-ignored; the
  switch off refuses.
- `name_items`: one candidate is named; several are skipped and listed; none is skipped; a photo
  already the person's (confirmed face, or a linked Picasa face) is skipped even with one
  unnamed face — the case that would otherwise name a stranger; an ignored face is not a
  candidate; the list is capped with a count.
- `remove_from_person`: confirmed and suggested faces of the person are rejected; another
  person's face on the same photo is untouched; a photo the person is on through Picasa stays
  and is counted; refused for an unnamed group.
- `viewer_item`: `faceId` on a detection's plate, on a linked Picasa plate with a confirmed
  detection beneath, `null` on an unlinked contact's plate; an unnamed Picasa outline carries the
  detection beneath it.
- The dialog's factory and the toast wording: pure tests. The menus, the hit test's wiring and
  the clickable outlines are covered by `svelte-check`, a screenshot of the dialog and the smoke
  checklist; the hit test itself is a pure function with tests (point in a face as drawn,
  rotated and cropped photos through `faceBox`).

## Not in this design

- Drawing a face rectangle by hand for a face photon did not find.
- Naming Picasa's faces (it would mean writing `.picasa.ini`).
- Undo.
