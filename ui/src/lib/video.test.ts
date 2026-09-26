import { describe, expect, it } from 'vitest';
import { formatDuration, mediaSupported, posterTime, videoUrl } from './video';

describe('posterTime', () => {
  it('is a second in, or a tenth of a short clip', () => {
    expect(posterTime(83)).toBe(1);
    expect(posterTime(4)).toBeCloseTo(0.4);
  });
  it('is the start when the duration is unknown', () => {
    for (const d of [NaN, Infinity, 0, -1]) expect(posterTime(d)).toBe(0);
  });
});

describe('formatDuration', () => {
  it('reads as a player shows it', () => {
    expect(formatDuration(0)).toBe('0:00');
    expect(formatDuration(9_400)).toBe('0:09');
    expect(formatDuration(83_000)).toBe('1:23');
    expect(formatDuration(3_723_000)).toBe('1:02:03');
  });
});

describe('mediaSupported', () => {
  it('asks for MP4, whose demuxer ships with the audio sink whose absence crashes WebKit', () => {
    const asked: string[] = [];
    expect(mediaSupported((t) => (asked.push(t), 'maybe'))).toBe(true);
    expect(asked).toEqual(['video/mp4']);
    expect(mediaSupported(() => '')).toBe(false);
  });
});

it('videoUrl joins the base and the id', () => {
  expect(videoUrl('http://127.0.0.1:5/abc', 12)).toBe('http://127.0.0.1:5/abc/video/12');
});
