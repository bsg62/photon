import { describe, expect, it } from 'vitest';
import { formatCaption, formatSize } from './caption';

const item = {
  fileName: 'IMG_1234.JPG',
  takenAt: 1_718_454_645, // 2024-06-15 12:30:45, a naive local time stored as UTC seconds
  width: 4000,
  height: 3000,
  orientation: 1,
  size: 3_355_443,
};

describe('formatSize', () => {
  it('uses KB below a megabyte and MB with one decimal above', () => {
    expect(formatSize(0)).toBe('0 KB');
    expect(formatSize(512)).toBe('1 KB');
    expect(formatSize(345_678)).toBe('338 KB');
    expect(formatSize(1_048_576)).toBe('1.0 MB');
    expect(formatSize(3_355_443)).toBe('3.2 MB');
  });
});

describe('formatCaption', () => {
  it('lists name, capture time, resolution, size and position, like Picasa', () => {
    expect(formatCaption(item, { index: 12, count: 240 }, 'en-US')).toBe(
      'IMG_1234.JPG · Jun 15, 2024, 12:30 PM · 4000 × 3000 · 3.2 MB · (12 / 240)',
    );
  });

  it('formats the capture time as stored, not shifted into the viewer’s zone', () => {
    // `takenAt` is the camera's wall-clock time stored as if it were UTC (see
    // metadata.rs). Rendering it through the local zone would show a different hour on
    // every machine the library is opened on.
    const caption = formatCaption({ ...item, takenAt: 0 }, { index: 1, count: 1 }, 'en-US');
    expect(caption).toContain('Jan 1, 1970, 12:00 AM');
  });

  it('shows the displayed resolution, with the orientation applied', () => {
    const caption = formatCaption({ ...item, orientation: 6 }, { index: 1, count: 1 }, 'en-US');
    expect(caption).toContain('3000 × 4000');
  });

  it('leaves the position out when there is nothing to number', () => {
    const caption = formatCaption(item, { index: 0, count: 0 }, 'en-US');
    expect(caption).toBe('IMG_1234.JPG · Jun 15, 2024, 12:30 PM · 4000 × 3000 · 3.2 MB');
  });
});
