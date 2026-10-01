import { describe, expect, it } from 'vitest';
import type { SavedSearch } from './api';
import { canSaveSearch, defaultSearchName, onThisDayLabel, onThisDayQuery, savedSearchFor } from './searches';

function saved(id: number, name: string, query: string): SavedSearch {
  return { id, name, query, createdMs: 0 };
}

const SEARCHES = [
  saved(1, 'Canon', 'camera:canon'),
  saved(2, 'Either', 'lake OR pond'),
];

describe('savedSearchFor', () => {
  it('finds the saved search whose query the box already holds', () => {
    expect(savedSearchFor(SEARCHES, 'camera:canon')?.id).toBe(1);
  });

  it('ignores whitespace around the typed query, as saving does', () => {
    expect(savedSearchFor(SEARCHES, '  camera:canon  ')?.id).toBe(1);
  });

  it('is undefined for a query nobody saved', () => {
    expect(savedSearchFor(SEARCHES, 'camera:nikon')).toBeUndefined();
  });

  it('is undefined for an empty box even if a saved search were blank', () => {
    expect(savedSearchFor([saved(9, 'Blank', '')], '   ')).toBeUndefined();
  });

  // `OR` is an operator only in capitals, so these are two different searches.
  it('tells a capitalised operator from a lowercase word', () => {
    expect(savedSearchFor(SEARCHES, 'lake or pond')).toBeUndefined();
    expect(savedSearchFor(SEARCHES, 'lake OR pond')?.id).toBe(2);
  });
});

describe('canSaveSearch', () => {
  it('is false for an empty box', () => {
    expect(canSaveSearch(SEARCHES, '   ')).toBe(false);
  });

  it('is false for a query already saved, so the button is inert rather than a toggle', () => {
    expect(canSaveSearch(SEARCHES, 'camera:canon')).toBe(false);
  });

  it('is true for a new query', () => {
    expect(canSaveSearch(SEARCHES, 'iso400')).toBe(true);
  });
});

describe('defaultSearchName', () => {
  it('offers the query itself, trimmed', () => {
    expect(defaultSearchName('  camera:canon 2019 ')).toBe('camera:canon 2019');
  });
});

describe('onThisDayQuery', () => {
  it('names the local month and day, two digits each, as the grammar wants them', () => {
    expect(onThisDayQuery(new Date(2026, 6, 4, 12))).toBe('on:07-04');
    expect(onThisDayQuery(new Date(2026, 11, 31, 23, 59))).toBe('on:12-31');
    expect(onThisDayQuery(new Date(2024, 1, 29, 0, 0))).toBe('on:02-29');
  });

  it('labels the day without a year', () => {
    expect(onThisDayLabel(new Date(2026, 6, 14, 12), 'en-GB')).toBe('14 July');
  });
});
