import { describe, expect, it } from 'vitest';
import { createVideoPlayer, fractionAt, SEEK_STEP_S, type PlayerMedia, type VideoPrefs } from './video-player.svelte';

/** Enough of an `HTMLVideoElement` to drive the player: plain fields, and events fired by
 *  hand the way the element fires them. */
function fakeMedia(duration = 100): PlayerMedia & { fire: (type: string) => void; listening: () => number } {
  const listeners = new Map<string, Set<() => void>>();
  const media = {
    paused: true,
    currentTime: 0,
    duration,
    volume: 1,
    muted: false,
    loop: false,
    play: async () => {
      media.paused = false;
      media.fire('play');
    },
    pause: () => {
      media.paused = true;
      media.fire('pause');
    },
    addEventListener: (type: string, fn: () => void) => {
      if (!listeners.has(type)) listeners.set(type, new Set());
      listeners.get(type)!.add(fn);
    },
    removeEventListener: (type: string, fn: () => void) => listeners.get(type)?.delete(fn),
    fire: (type: string) => listeners.get(type)?.forEach((fn) => fn()),
    listening: () => [...listeners.values()].reduce((n, set) => n + set.size, 0),
  };
  return media;
}

const prefs = (): VideoPrefs => ({ loop: false, muted: false, volume: 1 });
const key = (k: string, mods: Partial<KeyboardEvent> = {}) =>
  ({ key: k, shiftKey: false, ctrlKey: false, metaKey: false, altKey: false, ...mods }) as KeyboardEvent;

describe('createVideoPlayer', () => {
  it('toggles play and pause, and follows the element when it changes on its own', async () => {
    const player = createVideoPlayer(prefs());
    const media = fakeMedia();
    player.attach(media);
    await player.toggle();
    expect(media.paused).toBe(false);
    expect(player.playing).toBe(true);
    await player.toggle();
    expect(media.paused).toBe(true);
    // The video reaching its end pauses it without the player asking.
    media.paused = false;
    media.fire('play');
    media.paused = true;
    media.fire('pause');
    expect(player.playing).toBe(false);
  });

  it('seeks by the step and never past either end', () => {
    const player = createVideoPlayer(prefs());
    const media = fakeMedia(12);
    player.attach(media);
    media.currentTime = 3;
    player.seekBy(-SEEK_STEP_S);
    expect(media.currentTime).toBe(0);
    player.seekBy(SEEK_STEP_S);
    expect(media.currentTime).toBe(SEEK_STEP_S);
    player.seekBy(SEEK_STEP_S * 10);
    expect(media.currentTime).toBe(12);
  });

  it('does not seek while the length is unknown', () => {
    const player = createVideoPlayer(prefs());
    const media = fakeMedia(NaN);
    player.attach(media);
    player.seekBy(SEEK_STEP_S);
    player.seekTo(0.5);
    expect(media.currentTime).toBe(0);
  });

  it('reads the time and length from the element', () => {
    const player = createVideoPlayer(prefs());
    const media = fakeMedia(83);
    player.attach(media);
    media.currentTime = 20;
    media.fire('timeupdate');
    expect(player.time).toBe(20);
    expect(player.duration).toBe(83);
    media.duration = 90;
    media.fire('durationchange');
    expect(player.duration).toBe(90);
  });

  it('carries loop, mute and volume over to the next video', () => {
    const shared = prefs();
    const first = createVideoPlayer(shared);
    first.attach(fakeMedia());
    first.toggleLoop();
    // Volume before mute: turning the volume up unmutes, by design (next test).
    first.setVolume(0.4);
    first.toggleMute();
    // The next video is a fresh element under a fresh player, as the viewer makes one per video.
    const next = fakeMedia();
    const second = createVideoPlayer(shared);
    second.attach(next);
    expect(next.loop).toBe(true);
    expect(next.muted).toBe(true);
    expect(next.volume).toBe(0.4);
    expect([second.loop, second.muted, second.volume]).toEqual([true, true, 0.4]);
  });

  it('clamps the volume, and turning it up unmutes', () => {
    const player = createVideoPlayer(prefs());
    const media = fakeMedia();
    player.attach(media);
    player.setVolume(7);
    expect(media.volume).toBe(1);
    player.setVolume(-1);
    expect(media.volume).toBe(0);
    player.toggleMute();
    player.setVolume(0.5);
    expect(media.muted).toBe(false);
  });

  it('answers Space, L, M and Shift+arrows, and leaves plain arrows to the viewer', () => {
    const player = createVideoPlayer(prefs());
    const media = fakeMedia(60);
    player.attach(media);
    media.currentTime = 10;
    expect(player.handleKey(key('ArrowRight', { shiftKey: true }))).toBe(true);
    expect(media.currentTime).toBe(10 + SEEK_STEP_S);
    expect(player.handleKey(key('ArrowLeft', { shiftKey: true }))).toBe(true);
    expect(media.currentTime).toBe(10);
    expect(player.handleKey(key('l'))).toBe(true);
    expect(media.loop).toBe(true);
    expect(player.handleKey(key('M'))).toBe(true);
    expect(media.muted).toBe(true);
    expect(player.handleKey(key(' '))).toBe(true);
    expect(player.handleKey(key('ArrowRight'))).toBe(false);
    expect(player.handleKey(key('l', { ctrlKey: true }))).toBe(false);
  });

  it('scrubs, and Escape puts the video back where the drag began', () => {
    const player = createVideoPlayer(prefs());
    const media = fakeMedia(100);
    player.attach(media);
    media.currentTime = 30;
    player.beginScrub();
    player.scrubTo(0.8);
    expect(media.currentTime).toBe(80);
    expect(player.scrubbing).toBe(true);
    player.abandonScrub();
    expect(media.currentTime).toBe(30);
    expect(player.scrubbing).toBe(false);
    player.beginScrub();
    player.scrubTo(1.4);
    player.endScrub();
    expect(media.currentTime).toBe(100);
    expect(player.scrubbing).toBe(false);
  });

  it('stops listening to an element it was detached from', () => {
    // The viewer makes a new element per video; a player still subscribed to the last one
    // keeps it (and its decoder) reachable for as long as the player lives.
    const player = createVideoPlayer(prefs());
    const old = fakeMedia();
    player.attach(old);
    expect(old.listening()).toBeGreaterThan(0);
    player.attach(fakeMedia(10));
    expect(old.listening()).toBe(0);
    // And the detach an effect's teardown calls lets go of the current one.
    const current = fakeMedia(10);
    const detach = player.attach(current);
    detach();
    expect(current.listening()).toBe(0);
  });
});

describe('fractionAt', () => {
  it('is where along the bar the pointer is, clamped to the bar', () => {
    const rect = { left: 100, width: 200 };
    expect(fractionAt(150, rect)).toBe(0.25);
    expect(fractionAt(50, rect)).toBe(0);
    expect(fractionAt(900, rect)).toBe(1);
    expect(fractionAt(150, { left: 0, width: 0 })).toBe(0);
  });
});
