import { describe, expect, it } from 'vitest';
import type { TagCount, TagRule } from './api';
import { filterTags, renameCheck, ruleLabel } from './tags';

const tags: TagCount[] = [
  { tag: 'Beach', count: 3 },
  { tag: 'holiday', count: 1 },
];

const rules: TagRule[] = [{ tag: 'junk', target: null }];

describe('renameCheck', () => {
  it.each([
    ['a blank name is refused', 'holiday', '   ', 'blank'],
    ['the same name is no change', 'holiday', ' holiday ', 'same'],
    ['an existing name merges', 'holiday', 'Beach', 'merge'],
    ['names match exactly, so a case change is a merge only onto that exact name', 'holiday', 'beach', 'ok'],
    ['a new name renames', 'holiday', 'vacation', 'ok'],
    ['a name with a rule of its own is revived, which asks first', 'holiday', ' junk', 'revive'],
  ] as const)('%s', (_, from, to, expected) => {
    expect(renameCheck(from, to, tags, rules)).toBe(expected);
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
