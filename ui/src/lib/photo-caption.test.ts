import { describe, expect, it } from 'vitest';
import { photoCaptionLine } from './photo-caption';

describe('photoCaptionLine', () => {
  it('is the caption on one line, runs of whitespace and line breaks collapsed', () => {
    expect(photoCaptionLine("Grandma's 80th,\n  Lisbon")).toBe("Grandma's 80th, Lisbon");
    expect(photoCaptionLine('  Lisbon\t\r\n')).toBe('Lisbon');
  });

  it('is nothing for no caption or only whitespace', () => {
    expect(photoCaptionLine(null)).toBeNull();
    expect(photoCaptionLine('')).toBeNull();
    expect(photoCaptionLine(' \n\t ')).toBeNull();
  });
});
