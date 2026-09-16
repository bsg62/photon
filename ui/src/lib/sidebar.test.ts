import { describe, expect, it } from 'vitest';
import { clampSidebarWidth, SIDEBAR_MIN } from './sidebar';

describe('clampSidebarWidth', () => {
  it('keeps a width inside the bounds', () => {
    expect(clampSidebarWidth(300, 1200)).toBe(300);
  });

  it('stops at half the window', () => {
    expect(clampSidebarWidth(900, 1200)).toBe(600);
  });

  it('stops at the minimum', () => {
    expect(clampSidebarWidth(20, 1200)).toBe(SIDEBAR_MIN);
  });

  it('prefers the minimum when the window is too narrow for both', () => {
    expect(clampSidebarWidth(250, 200)).toBe(SIDEBAR_MIN);
  });
});
