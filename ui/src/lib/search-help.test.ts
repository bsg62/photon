import { describe, expect, it } from 'vitest';
import parser from '../../../crates/photon-core/src/search.rs?raw';
import { insertTerm, SEARCH_HELP } from './search-help';

/** `Query::terms`, the one function that reads a token: everything from its signature to
 *  the next one. The tests further down the file name prefixes too, and must not count. */
const terms = parser.slice(parser.indexOf('fn terms('), parser.indexOf('pub fn needs('));

/** Every prefix the parser knows, as it is typed: `camera:`, `is:`, `size:`. */
const prefixes = [...terms.matchAll(/prefixed\("([a-z]+:)"\)/g)].map((m) => m[1]);

/** The values a prefix with a fixed set takes: the arms of the `match` that follows it. */
function valuesOf(prefix: string): string[] {
  const from = terms.indexOf(`prefixed("${prefix}")`);
  const rest = terms.slice(from + 1);
  const block = rest.slice(0, rest.indexOf('prefixed("'));
  return [...block.matchAll(/"([a-z]+)" =>/g)].map((m) => m[1]);
}

/** The prefixes that take one of a fixed set of words rather than a value of the user's. */
const FIXED = ['is:', 'has:'];

const listed = SEARCH_HELP.flatMap((g) => g.entries)
  .filter((e) => e.insert)
  .map((e) => e.text);

describe('the search help', () => {
  it('reads the grammar out of the parser', () => {
    // The checks below pass on a parser they could read nothing from. These are what it
    // holds today; a new prefix or value belongs here and in the list.
    expect(terms.length).toBeGreaterThan(0);
    expect([...prefixes].sort()).toEqual(
      ['album:', 'aperture:', 'camera:', 'faces:', 'focal:', 'folder:', 'from:', 'has:', 'is:', 'iso:', 'lens:', 'mp:', 'near:', 'on:', 'person:', 'size:', 'tag:', 'to:'].sort(),
    );
    expect(valuesOf('is:').sort()).toEqual(
      ['duplicate', 'edited', 'landscape', 'photo', 'portrait', 'square', 'starred', 'video'].sort(),
    );
    expect(valuesOf('has:').sort()).toEqual(['album', 'caption', 'face', 'gps', 'person', 'tag'].sort());
  });

  it('lists everything the parser understands', () => {
    const known = [
      ...prefixes.filter((p) => !FIXED.includes(p)),
      ...FIXED.flatMap((p) => valuesOf(p).map((v) => p + v)),
    ];
    expect(known.filter((term) => !listed.includes(term))).toEqual([]);
  });

  it('offers nothing the parser would drop', () => {
    const known = new Set([
      ...prefixes.filter((p) => !FIXED.includes(p)),
      ...FIXED.flatMap((p) => valuesOf(p).map((v) => p + v)),
    ]);
    expect(listed.filter((term) => !known.has(term))).toEqual([]);
  });

  it('says what every entry finds, once', () => {
    const texts = SEARCH_HELP.flatMap((g) => g.entries).map((e) => e.text);
    expect(new Set(texts).size).toBe(texts.length);
    for (const g of SEARCH_HELP) {
      expect(g.title).not.toBe('');
      expect(g.entries.length, g.title).toBeGreaterThan(0);
      for (const e of g.entries) expect(e.does, e.text).not.toBe('');
    }
  });
});

describe('insertTerm', () => {
  it('is the term alone in an empty box', () => {
    expect(insertTerm('', 'is:starred')).toBe('is:starred');
    expect(insertTerm('   ', 'camera:')).toBe('camera:');
  });

  it('goes after what is there, one space between', () => {
    expect(insertTerm('lisbon', 'is:starred')).toBe('lisbon is:starred');
    // A space already typed is not doubled.
    expect(insertTerm('lisbon ', 'tag:')).toBe('lisbon tag:');
    expect(insertTerm('lake OR pond', 'has:gps')).toBe('lake OR pond has:gps');
  });

  it('closes a phrase left open, so the term is not read as part of it', () => {
    expect(insertTerm('"summer hike', 'is:starred')).toBe('"summer hike" is:starred');
    expect(insertTerm('lake "summer hi ', 'tag:')).toBe('lake "summer hi" tag:');
    // A closed phrase, and two of them, are left alone.
    expect(insertTerm('"summer hike"', 'is:starred')).toBe('"summer hike" is:starred');
    expect(insertTerm('"a b" "c d"', 'on:')).toBe('"a b" "c d" on:');
  });

  it('leaves what was typed as it was', () => {
    // Leading space and inner spacing are the user's, a quoted phrase's included.
    expect(insertTerm(' "summer  hike"', 'on:')).toBe(' "summer  hike" on:');
  });
});
