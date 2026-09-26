import { describe, expect, it } from 'vitest';
import viewer from '../components/Viewer.svelte?raw';

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
