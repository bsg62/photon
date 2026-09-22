import { describe, expect, it } from 'vitest';
import { keepCopiesName, showCopies, showCopiesLabel } from './copies';

describe('showCopies', () => {
  function spyDeps(at: number | null = 3) {
    const order: string[] = [];
    return {
      order,
      deps: {
        cancelSearch: () => order.push('cancel'),
        setCopiesView: (id: number) => {
          order.push(`view:${id}`);
          return Promise.resolve();
        },
        offsetOfItem: (id: number) => {
          order.push(`find:${id}`);
          return Promise.resolve(at);
        },
        select: (offset: number, itemId: number) => order.push(`select:${offset}:${itemId}`),
      },
    };
  }

  it('switches to the view before looking the photo up, so the offset is against its index', async () => {
    const { order, deps } = spyDeps();
    await showCopies(42, deps);
    expect(order).toEqual(['cancel', 'view:42', 'find:42', 'select:3:42']);
  });

  it('selects nothing when the view does not hold the photo', async () => {
    const { order, deps } = spyDeps(null);
    await showCopies(42, deps);
    expect(order).toEqual(['cancel', 'view:42', 'find:42']);
  });
});

describe('keepCopiesName', () => {
  it('keeps the name it had when the photo has since left the library', () => {
    expect(keepCopiesName({ id: 7, fileName: 'a.jpg' }, { id: 7, fileName: '' })).toEqual({ id: 7, fileName: 'a.jpg' });
  });
  it('does not carry one photo’s name onto another', () => {
    expect(keepCopiesName({ id: 7, fileName: 'a.jpg' }, { id: 8, fileName: '' })).toEqual({ id: 8, fileName: '' });
  });
  it('takes a fresh name, and leaves no view as no view', () => {
    expect(keepCopiesName({ id: 7, fileName: 'a.jpg' }, { id: 7, fileName: 'b.jpg' })).toEqual({ id: 7, fileName: 'b.jpg' });
    expect(keepCopiesName({ id: 7, fileName: 'a.jpg' }, null)).toBeNull();
  });
});

describe('showCopiesLabel', () => {
  it('counts in the singular and the plural', () => {
    expect(showCopiesLabel(1)).toBe('Show 1 duplicate');
    expect(showCopiesLabel(3)).toBe('Show 3 duplicates');
    expect(showCopiesLabel(1200)).toBe(`Show ${(1200).toLocaleString()} duplicates`);
  });
});
