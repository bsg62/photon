import { describe, expect, it } from 'vitest';
import { menuOwnsKey, menuStep } from './menu-keys';

describe('menuStep', () => {
  it('moves down and up one item', () => {
    expect(menuStep('ArrowDown', 0, 4)).toBe(1);
    expect(menuStep('ArrowUp', 2, 4)).toBe(1);
  });

  it('wraps at both ends', () => {
    expect(menuStep('ArrowDown', 3, 4)).toBe(0);
    expect(menuStep('ArrowUp', 0, 4)).toBe(3);
  });

  it('enters the menu from the menu itself: down at the top, up at the bottom', () => {
    expect(menuStep('ArrowDown', -1, 4)).toBe(0);
    expect(menuStep('ArrowUp', -1, 4)).toBe(3);
  });

  it('goes to either end', () => {
    expect(menuStep('Home', 2, 4)).toBe(0);
    expect(menuStep('End', 1, 4)).toBe(3);
  });

  it('stays on the only item', () => {
    expect(menuStep('ArrowDown', 0, 1)).toBe(0);
    expect(menuStep('ArrowUp', 0, 1)).toBe(0);
  });

  it('answers no other key, and nothing in a menu with no item to land on', () => {
    expect(menuStep('Enter', 0, 4)).toBeNull();
    expect(menuStep('ArrowLeft', 0, 4)).toBeNull();
    expect(menuStep('a', 0, 4)).toBeNull();
    expect(menuStep('ArrowDown', -1, 0)).toBeNull();
    expect(menuStep('Home', -1, 0)).toBeNull();
  });
});

describe('menuOwnsKey', () => {
  const key = (k: string, over: Partial<Parameters<typeof menuOwnsKey>[0]> = {}) => ({
    key: k,
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    ...over,
  });

  it('keeps every plain key from what is behind the menu', () => {
    for (const k of ['h', 'r', 's', ' ', 'Enter', 'ArrowLeft', 'ArrowDown', 'Backspace', '?', '.']) {
      expect(menuOwnsKey(key(k)), k).toBe(true);
    }
  });

  it('leaves Escape to whoever opened the menu', () => {
    expect(menuOwnsKey(key('Escape'))).toBe(false);
  });

  it('leaves the function keys to the app: F11 is the way out of fullscreen', () => {
    expect(menuOwnsKey(key('F11'))).toBe(false);
    expect(menuOwnsKey(key('F1'))).toBe(false);
    // A letter that happens to be F is a plain key like any other.
    expect(menuOwnsKey(key('F'))).toBe(true);
    expect(menuOwnsKey(key('f'))).toBe(true);
  });

  it('leaves a chord to the app and the system', () => {
    expect(menuOwnsKey(key('a', { ctrlKey: true }))).toBe(false);
    expect(menuOwnsKey(key('c', { metaKey: true }))).toBe(false);
    expect(menuOwnsKey(key('F4', { altKey: true }))).toBe(false);
  });
});
