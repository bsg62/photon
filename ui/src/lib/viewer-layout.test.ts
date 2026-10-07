import { describe, expect, it } from 'vitest';
import viewer from '../components/Viewer.svelte?raw';
import { canStep, fromPhotoCentre, INFO_GAP, INFO_RIGHT, INFO_WIDTH, infoRoom, photoArea } from './viewer-layout';

/** The viewer's own style block, where its layout rules live. */
const styles = viewer.slice(viewer.indexOf('<style>'));

/** The declarations of one rule, by its exact selector. */
function rule(selector: string): string {
  const at = styles.indexOf(`\n  ${selector} {`);
  expect(at, `no "${selector}" rule in Viewer.svelte`).toBeGreaterThan(-1);
  return styles.slice(at, styles.indexOf('}', at));
}

describe("the viewer's video box", () => {
  // A `<video>` is a replaced element: absolutely positioned with `height: auto`, its height
  // comes from its own aspect ratio and a `bottom` inset is ignored. A portrait video then
  // came out as wide as the window and several screens tall, pinned to the top - most of it
  // cut off (measured in headless Chromium: 1280x2276 in a 713px-high viewport). An explicit
  // height is what makes `object-fit: contain` fit the picture into the space left above the
  // controls.
  it('has an explicit height, never auto', () => {
    const video = rule('video.full');
    expect(video).toMatch(/height:\s*calc\(100% - \d+px\)/);
    expect(video).not.toMatch(/height:\s*auto/);
  });
});

describe('the room the info panel takes from the photo', () => {
  it('is nothing while the panel is closed', () => {
    expect(infoRoom(false)).toBe(0);
  });

  it('is the panel, its distance from the edge and the gap beside it while open', () => {
    expect(infoRoom(true)).toBe(INFO_WIDTH + INFO_RIGHT + INFO_GAP);
  });
});

describe('the area the photo is fitted into', () => {
  it('is the whole viewer with the panel closed', () => {
    expect(photoArea(1280, 800, false)).toEqual({ width: 1280, height: 800 });
  });

  it('is what the panel leaves, at the viewer\'s full height', () => {
    expect(photoArea(1280, 800, true)).toEqual({ width: 1280 - infoRoom(true), height: 800 });
  });

  it('is never negative, in a viewer not yet laid out', () => {
    expect(photoArea(0, 0, true)).toEqual({ width: 0, height: 0 });
  });
});

describe('a pointer measured from the middle of the photo\'s area', () => {
  const viewer = { left: 0, top: 0, width: 1280, height: 800 };

  it('is measured from the middle of the viewer with the panel closed', () => {
    expect(fromPhotoCentre(640, 400, viewer, false)).toEqual({ x: 0, y: 0 });
    expect(fromPhotoCentre(740, 350, viewer, false)).toEqual({ x: 100, y: -50 });
  });

  // The zoom holds still the point it is given, and the pan it answers with is measured from
  // the middle of the area the photo is in: measured from the middle of the window instead,
  // a double-click beside the open panel zoomed to a place half the panel's room away.
  it('is measured from the middle of what the panel leaves while it is open', () => {
    const middle = (1280 - infoRoom(true)) / 2;
    expect(fromPhotoCentre(middle, 400, viewer, true)).toEqual({ x: 0, y: 0 });
    expect(fromPhotoCentre(640, 400, viewer, true)).toEqual({ x: infoRoom(true) / 2, y: 0 });
  });

  it('allows for a viewer that does not start at the window\'s corner', () => {
    expect(fromPhotoCentre(110, 70, { left: 10, top: 20, width: 200, height: 100 }, false)).toEqual({ x: 0, y: 0 });
  });
});

describe('whether there is a photo to step to', () => {
  it('has none before the first photo and none after the last', () => {
    expect(canStep(-1, 0, 5, false)).toBe(false);
    expect(canStep(1, 4, 5, false)).toBe(false);
  });

  it('has one in between', () => {
    expect(canStep(1, 0, 5, false)).toBe(true);
    expect(canStep(-1, 4, 5, false)).toBe(true);
    expect(canStep(-1, 2, 5, false)).toBe(true);
  });

  it('has none either way with one photo, or with none', () => {
    expect(canStep(1, 0, 1, false)).toBe(false);
    expect(canStep(-1, 0, 1, false)).toBe(false);
    expect(canStep(1, 0, 0, false)).toBe(false);
  });

  // A slideshow wraps round at both ends, so either way always leads somewhere - but for a
  // show of one photo, which has no other to go to.
  it('always has one in a slideshow of more than one photo', () => {
    expect(canStep(1, 4, 5, true)).toBe(true);
    expect(canStep(-1, 0, 5, true)).toBe(true);
    expect(canStep(1, 0, 1, true)).toBe(false);
  });

  // A photo that has left the view holds no place in it: the one at its offset is its
  // right-hand neighbour, so there is a next while there is anything there at all.
  it('counts the photo at its own offset as the next one for a photo that has left the view', () => {
    expect(canStep(1, 4, 5, false, true)).toBe(true);
    expect(canStep(1, 5, 5, false, true)).toBe(false);
    expect(canStep(-1, 0, 5, false, true)).toBe(false);
    expect(canStep(-1, 1, 5, false, true)).toBe(true);
  });
});
