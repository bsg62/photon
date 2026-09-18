import { describe, expect, it } from 'vitest';
import { cameraName, cameraRows, fieldQuery, formatAperture, formatExposure, formatFocal, formatIso } from './exif';

describe('cameraName', () => {
  it('does not repeat the make when the model already names it', () => {
    expect(cameraName('Canon', 'Canon EOS 5D Mark IV')).toBe('Canon EOS 5D Mark IV');
    expect(cameraName('NIKON CORPORATION', 'NIKON D750')).toBe('NIKON D750');
    expect(cameraName('Apple', 'iPhone 15 Pro')).toBe('Apple iPhone 15 Pro');
  });

  it('copes with either half missing', () => {
    expect(cameraName('Canon', null)).toBe('Canon');
    expect(cameraName(null, 'EOS 5D')).toBe('EOS 5D');
    expect(cameraName(null, null)).toBeNull();
    expect(cameraName('  ', '')).toBeNull();
  });
});

describe('the exposure formatters', () => {
  it('spell focal length, aperture and ISO as a photographer reads them', () => {
    expect(formatFocal(50)).toBe('50 mm');
    expect(formatFocal(18.5)).toBe('18.5 mm');
    expect(formatFocal(4.25)).toBe('4.3 mm');
    expect(formatAperture(1.8)).toBe('f/1.8');
    expect(formatAperture(11)).toBe('f/11');
    expect(formatIso(400)).toBe('ISO 400');
  });

  it('spell shutter speed as a fraction below a second and seconds above', () => {
    expect(formatExposure(1 / 250)).toBe('1/250 s');
    expect(formatExposure(0.004)).toBe('1/250 s');
    expect(formatExposure(1 / 4)).toBe('1/4 s');
    expect(formatExposure(0.5)).toBe('0.5 s');
    expect(formatExposure(0.3)).toBe('0.3 s');
    expect(formatExposure(1)).toBe('1 s');
    expect(formatExposure(2.5)).toBe('2.5 s');
    expect(formatExposure(30)).toBe('30 s');
    expect(formatExposure(0)).toBe('');
  });
});

describe('cameraRows', () => {
  it('lists camera, lens and one exposure line', () => {
    expect(
      cameraRows({
        make: 'Canon',
        model: 'Canon EOS 5D',
        lens: 'EF50mm f/1.8 STM',
        focalMm: 50,
        aperture: 1.8,
        exposureS: 0.004,
        iso: 400,
      }),
    ).toEqual([
      { label: 'Camera', value: 'Canon EOS 5D', search: 'camera:"Canon EOS 5D"' },
      { label: 'Lens', value: 'EF50mm f/1.8 STM', search: 'lens:"EF50mm f/1.8 STM"' },
      { label: 'Exposure', value: '50 mm · f/1.8 · 1/250 s · ISO 400' },
    ]);
  });

  it('is empty for a photo with no camera data, and partial for a partial one', () => {
    const none = { make: null, model: null, lens: null, focalMm: null, aperture: null, exposureS: null, iso: null };
    expect(cameraRows(none)).toEqual([]);
    expect(cameraRows({ ...none, iso: 100 })).toEqual([{ label: 'Exposure', value: 'ISO 100' }]);
  });
});

describe('fieldQuery', () => {
  it('quotes the value and drops a quote the grammar could not escape', () => {
    expect(fieldQuery('camera', 'NIKON D750')).toBe('camera:"NIKON D750"');
    expect(fieldQuery('lens', ' 7" tele ')).toBe('lens:"7  tele"');
  });
});
