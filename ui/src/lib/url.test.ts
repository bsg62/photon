import { describe, expect, it } from 'vitest';
import { mediaUrl } from './url';

describe('mediaUrl', () => {
  it('uses the custom scheme on Linux and macOS', () => {
    expect(mediaUrl('thumb/1/grid/00ff', false)).toBe('photon://localhost/thumb/1/grid/00ff');
  });
  it('uses the http localhost form on Windows', () => {
    expect(mediaUrl('image/7', true)).toBe('http://photon.localhost/image/7');
  });
});
