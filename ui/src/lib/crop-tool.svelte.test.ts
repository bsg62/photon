import { describe, expect, it } from 'vitest';
import { ASPECTS } from './crop';
import { createCropTool } from './crop-tool.svelte';

const square = ASPECTS.findIndex((a) => a.label === '1:1');

describe('createCropTool', () => {
  it('opens on the crop the photo already has, unlocked', () => {
    const tool = createCropTool();
    tool.begin([32768, 0, 65535, 65535], 300, 200);
    expect(tool.active).toBe(true);
    expect(tool.rect.left).toBeCloseTo(0.5, 4);
    expect(tool.aspect).toBe(0);
    expect(tool.wire()).toEqual([32768, 0, 65535, 65535]);
  });

  it('opens on the whole picture for an uncropped photo, which saves as no crop', () => {
    const tool = createCropTool();
    tool.begin(null, 300, 200);
    expect(tool.wire()).toBeNull();
  });

  it('drags from where the press began, so the edge does not eat the way back', () => {
    const tool = createCropTool();
    tool.begin([0, 0, 32768, 65535], 1000, 1000);
    tool.startDrag('e', 100, 100);
    // Far past the right edge, then back to 100px right of the press. Accumulated per
    // move, the rectangle would end 900px short; from the press it ends 100px out.
    tool.dragTo(2000, 100, 1000, 1000);
    expect(tool.rect.right).toBe(1);
    tool.dragTo(200, 100, 1000, 1000);
    expect(tool.rect.right).toBeCloseTo(0.6, 4);
    tool.endDrag();
    tool.dragTo(900, 100, 1000, 1000);
    expect(tool.rect.right).toBeCloseTo(0.6, 4);
  });

  it('reshapes to a chosen ratio in pixels, not in fractions', () => {
    const tool = createCropTool();
    tool.begin(null, 300, 200);
    tool.setAspect(square);
    // A square of a 3:2 picture is two thirds of its width and all of its height.
    expect(tool.rect.right - tool.rect.left).toBeCloseTo(2 / 3, 9);
    expect(tool.rect.bottom - tool.rect.top).toBeCloseTo(1, 9);

    tool.startDrag('se', 0, 0);
    tool.dragTo(-60, 0, 300, 200);
    const w = (tool.rect.right - tool.rect.left) * 300;
    const h = (tool.rect.bottom - tool.rect.top) * 200;
    expect(w).toBeCloseTo(h, 6);
  });

  it('clears back to the whole picture and unlocks', () => {
    const tool = createCropTool();
    tool.begin([1000, 1000, 30000, 30000], 300, 200);
    tool.setAspect(square);
    tool.clear();
    expect(tool.wire()).toBeNull();
    expect(tool.aspect).toBe(0);
  });

  it('ignores a ratio that does not exist and a drag over nothing', () => {
    const tool = createCropTool();
    tool.begin(null, 300, 200);
    tool.setAspect(99);
    expect(tool.aspect).toBe(0);
    tool.startDrag('e', 0, 0);
    tool.dragTo(-50, 0, 0, 0);
    expect(tool.wire()).toBeNull();
    tool.cancel();
    expect(tool.active).toBe(false);
  });
});
