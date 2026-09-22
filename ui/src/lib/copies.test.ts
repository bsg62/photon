import { describe, expect, it } from 'vitest';
import { copiesNotice, keepCopiesName, showCopies, showCopiesLabel } from './copies';

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
    expect(keepCopiesName({ id: 7, fileName: 'a.jpg', gone: false }, { id: 7, fileName: '', gone: true })).toEqual({
      id: 7,
      fileName: 'a.jpg',
      gone: true,
    });
  });
  it('does not carry one photo’s name onto another', () => {
    expect(keepCopiesName({ id: 7, fileName: 'a.jpg', gone: false }, { id: 8, fileName: '', gone: false })).toEqual({
      id: 8,
      fileName: '',
      gone: false,
    });
  });
  it('takes a fresh name, and leaves no view as no view', () => {
    expect(keepCopiesName({ id: 7, fileName: 'a.jpg', gone: false }, { id: 7, fileName: 'b.jpg', gone: false })).toEqual({
      id: 7,
      fileName: 'b.jpg',
      gone: false,
    });
    expect(keepCopiesName({ id: 7, fileName: 'a.jpg', gone: false }, null)).toBeNull();
  });
  it('always takes `gone` from `next`, never from `prev`, even while keeping the old name', () => {
    expect(keepCopiesName({ id: 7, fileName: 'a.jpg', gone: true }, { id: 7, fileName: '', gone: false })).toEqual({
      id: 7,
      fileName: 'a.jpg',
      gone: false,
    });
  });
});

describe('showCopiesLabel', () => {
  it('counts in the singular and the plural', () => {
    expect(showCopiesLabel(1)).toBe('Show 1 duplicate');
    expect(showCopiesLabel(3)).toBe('Show 3 duplicates');
    expect(showCopiesLabel(1200)).toBe(`Show ${(1200).toLocaleString()} duplicates`);
  });
});

describe('copiesNotice', () => {
  it('says nothing while the view is not open', () => {
    expect(copiesNotice(null, 0, undefined)).toBeNull();
  });

  it('reports the anchor as gone regardless of how many rows are left, with a name fallback', () => {
    expect(copiesNotice({ id: 7, fileName: 'a.jpg', gone: true }, 0, undefined)).toBe(
      'a.jpg is no longer in the library; its copies are under Duplicates.',
    );
    expect(copiesNotice({ id: 7, fileName: 'a.jpg', gone: true }, 2, 9)).toBe(
      'a.jpg is no longer in the library; its copies are under Duplicates.',
    );
    expect(copiesNotice({ id: 7, fileName: '', gone: true }, 0, undefined)).toBe(
      'This photo is no longer in the library; its copies are under Duplicates.',
    );
  });

  it('says there are no other copies when the view is empty', () => {
    expect(copiesNotice({ id: 7, fileName: 'a.jpg', gone: false }, 0, undefined)).toBe('No other copies of a.jpg any more.');
    expect(copiesNotice({ id: 7, fileName: '', gone: false }, 0, undefined)).toBe('No other copies of this photo any more.');
  });

  it('says there are no other copies when only the anchor itself is left', () => {
    expect(copiesNotice({ id: 7, fileName: 'a.jpg', gone: false }, 1, 7)).toBe('No other copies of a.jpg any more.');
  });

  it('says nothing when one row is left but it is not the anchor', () => {
    expect(copiesNotice({ id: 7, fileName: 'a.jpg', gone: false }, 1, 9)).toBeNull();
  });

  it('says nothing while the view still holds more than the anchor', () => {
    expect(copiesNotice({ id: 7, fileName: 'a.jpg', gone: false }, 2, 7)).toBeNull();
  });
});
