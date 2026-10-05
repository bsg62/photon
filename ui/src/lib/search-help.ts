/** One thing the search box understands, as the help beside it lists it. */
export interface SearchHelpEntry {
  /** The text as it is typed. With `insert`, a click puts it in the box: a whole term
   *  (`is:starred`), or a prefix the user finishes (`camera:`). Without, it is an example
   *  to read - a word, a phrase, a date - since the user's own is what belongs in the box. */
  text: string;
  insert?: true;
  does: string;
}

export interface SearchHelpGroup {
  title: string;
  entries: SearchHelpEntry[];
}

/** Everything the search box understands: what the help popover draws.
 *
 *  Written by hand from `Query::parse` in `crates/photon-core/src/search.rs`, and held to
 *  it by `search-help.test.ts`, which reads that file: a prefix, an `is:` or a `has:` the
 *  parser knows that is not here fails it, and so does one listed here that the parser does
 *  not know. What it cannot check is the wording, or the examples inside it.
 *
 *  Before this the grammar was one 900-character `title` on the input, which could be
 *  neither read at a glance nor copied from. */
export const SEARCH_HELP: SearchHelpGroup[] = [
  {
    title: 'Words',
    entries: [
      { text: 'lisbon tram', does: 'Both words, wherever they are: file or folder name, camera, lens, keyword, caption' },
      { text: 'lake OR pond', does: 'Either one. OR is written in capitals' },
      { text: '"summer hike"', does: 'The words together, in that order' },
      { text: '-draft', does: 'Without it. A hyphen works before anything below too: -tag:family, -has:tag' },
    ],
  },
  {
    title: 'When',
    entries: [
      { text: '2024-06', does: 'A year, month or day it was taken: 2024, 2024-06, 2024-06-14' },
      { text: 'from:', insert: true, does: 'From then on: from:2019-06' },
      { text: 'to:', insert: true, does: 'Up to and including then: to:2020' },
      { text: 'on:', insert: true, does: 'That day in any year: on:07-14' },
    ],
  },
  {
    title: 'In one place',
    entries: [
      { text: 'camera:', insert: true, does: 'The camera only: camera:canon' },
      { text: 'lens:', insert: true, does: 'The lens only: lens:50mm' },
      { text: 'tag:', insert: true, does: 'A keyword: tag:family' },
      { text: 'person:', insert: true, does: 'Someone named on the photo: person:anna' },
      { text: 'album:', insert: true, does: 'An album it is in: album:lisbon' },
      { text: 'folder:', insert: true, does: 'Its folder: folder:2019' },
    ],
  },
  {
    title: 'What it is',
    entries: [
      { text: 'is:starred', insert: true, does: 'Starred' },
      { text: 'is:edited', insert: true, does: 'Turned or cropped in photon' },
      { text: 'is:photo', insert: true, does: 'A photo, not a video' },
      { text: 'is:video', insert: true, does: 'A video' },
      { text: 'is:duplicate', insert: true, does: 'Has a copy or a look-alike in the library' },
      { text: 'is:portrait', insert: true, does: 'Taller than wide' },
      { text: 'is:landscape', insert: true, does: 'Wider than tall' },
      { text: 'is:square', insert: true, does: 'As wide as tall' },
    ],
  },
  {
    title: 'What it has',
    entries: [
      { text: 'has:tag', insert: true, does: 'Any keyword. -has:tag finds the untagged' },
      { text: 'has:caption', insert: true, does: 'A caption' },
      { text: 'has:album', insert: true, does: 'In at least one album' },
      { text: 'has:person', insert: true, does: 'Someone named on it' },
      { text: 'has:face', insert: true, does: 'A face, named or not' },
      { text: 'faces:', insert: true, does: 'That many faces: faces:2, or faces:3+ for three or more' },
      { text: 'has:gps', insert: true, does: 'Records where it was taken' },
      { text: 'near:', insert: true, does: 'Within a kilometre of a place: near:46.54,12.14 - or near:46.54,12.14,5km' },
    ],
  },
  {
    title: 'Numbers',
    entries: [
      { text: '50mm', does: 'A focal length as a word. So are f/1.8 and iso400' },
      { text: 'size:', insert: true, does: 'File size, with kb, mb or gb: size:>10mb' },
      { text: 'mp:', insert: true, does: 'Megapixels: mp:<2' },
      { text: 'iso:', insert: true, does: 'ISO: iso:>=1600' },
      { text: 'aperture:', insert: true, does: 'The f-number: aperture:<2' },
      { text: 'focal:', insert: true, does: 'Focal length in millimetres: focal:>100' },
    ],
  },
];

/** The box's text after a click on an entry: the term after what is already there, one
 *  space between. The search is every word at once, so adding narrows it - which is what a
 *  click on "is:starred" under a typed "lisbon" is asking for. */
export function insertTerm(query: string, term: string): string {
  const held = query.trimEnd();
  return held === '' ? term : `${held} ${term}`;
}
