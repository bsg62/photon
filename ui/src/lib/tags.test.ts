import { describe, expect, it } from 'vitest';
import type { TagCount } from './api';
import { filterTags, renameCheck, ruleLabel } from './tags';

const tags: TagCount[] = [
  { tag: 'Beach', count: 3 },
  { tag: 'holiday', count: 1 },
];

describe('renameCheck', () => {
  it.each([
    ['a blank name is refused', 'holiday', '   ', 'blank'],
    ['the same name is no change', 'holiday', ' holiday ', 'same'],
    ['an existing name merges', 'holiday', 'Beach', 'merge'],
    ['names match exactly, so a case change is a merge only onto that exact name', 'holiday', 'beach', 'ok'],
    ['a new name renames', 'holiday', 'vacation', 'ok'],
  ] as const)('%s', (_, from, to, expected) => {
    expect(renameCheck(from, to, tags)).toBe(expected);
  });
});

describe('filterTags', () => {
  it('matches a substring case-insensitively', () => {
    expect(filterTags(tags, 'BEA').map((t) => t.tag)).toEqual(['Beach']);
  });

  it('a blank filter keeps everything', () => {
    expect(filterTags(tags, '  ')).toEqual(tags);
  });
});

describe('ruleLabel', () => {
  it('shows a rename as an arrow', () => {
    expect(ruleLabel({ tag: 'holiday', target: 'vacation' })).toBe('holiday → vacation');
  });

  it('shows a removal', () => {
    expect(ruleLabel({ tag: 'junk', target: null })).toBe('junk — removed');
  });
});
