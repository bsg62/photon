import { describe, expect, it } from 'vitest';
import { move } from './nav';

const sections = [
  { folderId: 1, offset: 0, count: 5 },
  { folderId: 2, offset: 5, count: 3 },
];

describe('move', () => {
  it('steps left and right across the whole grid', () => {
    expect(move(0, 'ArrowLeft', sections, 2)).toBe(0);
    expect(move(4, 'ArrowRight', sections, 2)).toBe(5);
    expect(move(7, 'ArrowRight', sections, 2)).toBe(7);
    expect(move(3, 'Home', sections, 2)).toBe(0);
    expect(move(3, 'End', sections, 2)).toBe(7);
  });

  it('moves down by rows, into the next section at the same column', () => {
    expect(move(0, 'ArrowDown', sections, 2)).toBe(2);
    expect(move(3, 'ArrowDown', sections, 2)).toBe(4);
    expect(move(4, 'ArrowDown', sections, 2)).toBe(5);
    expect(move(6, 'ArrowDown', sections, 2)).toBe(7);
    expect(move(7, 'ArrowDown', sections, 2)).toBe(7);
  });

  it('moves up by rows, into the previous section’s last row', () => {
    expect(move(3, 'ArrowUp', sections, 2)).toBe(1);
    expect(move(5, 'ArrowUp', sections, 2)).toBe(4);
    expect(move(6, 'ArrowUp', sections, 2)).toBe(4);
    expect(move(1, 'ArrowUp', sections, 2)).toBe(1);
  });

  it('handles an empty grid', () => {
    expect(move(0, 'ArrowDown', [], 4)).toBe(0);
  });
});
