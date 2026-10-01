import { describe, expect, it } from 'vitest';
import type { ItemCopy, ItemDates } from './api';
import { cameraName, cameraRows, copyGroups, dateRows, fieldQuery, formatAperture, formatCoordinates, formatDimensions, formatExposure, formatFocal, formatIso, nearQuery } from './exif';

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

describe('formatDimensions', () => {
  it('spells width and height with the multiplication sign', () => {
    expect(formatDimensions(4000, 3000)).toBe('4000 × 3000');
  });
});

describe('copy groups', () => {
  it('splits copies into identical and look-alike, keeping order', () => {
    const copies: ItemCopy[] = [
      { id: 2, path: '/a/b.jpg', kind: 'identical', width: 4000, height: 3000 },
      { id: 3, path: '/c/d.jpg', kind: 'similar', width: 2048, height: 1536 },
      { id: 4, path: '/e/f.jpg', kind: 'similar', width: 800, height: 600 },
    ];
    expect(copyGroups(copies)).toEqual([
      { kind: 'identical', label: 'Identical', copies: [copies[0]] },
      { kind: 'similar', label: 'Looks the same', copies: [copies[1], copies[2]] },
    ]);
  });

  it('omits a group with no members', () => {
    const copies: ItemCopy[] = [
      { id: 2, path: '/a/b.jpg', kind: 'identical', width: 10, height: 10 },
    ];
    expect(copyGroups(copies).map((g) => g.kind)).toEqual(['identical']);
  });

  it('has nothing to show for a photo with no copies', () => {
    expect(copyGroups([])).toEqual([]);
  });
});

describe('dateRows', () => {
  // 2024-06-15 12:30 as the camera's wall clock (naive seconds) and as an instant (ms).
  const noon = 1_718_454_645;
  const none: ItemDates = { taken: null, digitized: null, edited: null, fileCreatedMs: null, fileModifiedMs: noon * 1000 };
  const rows = (dates: Partial<ItemDates>, timeZone = 'UTC') => dateRows({ ...none, ...dates }, 'en-GB', timeZone).map((r) => [r.label, r.value]);

  it('lists every date the photo has, the camera first, leaving out the ones it lacks', () => {
    expect(rows({ taken: noon, edited: noon + 86_400, fileCreatedMs: (noon + 3_600) * 1000 })).toEqual([
      ['Taken', '15 Jun 2024, 12:30'],
      ['Edited', '16 Jun 2024, 12:30'],
      ['File created', '15 Jun 2024, 13:30'],
      ['File modified', '15 Jun 2024, 12:30'],
    ]);
    expect(rows({})).toEqual([['File modified', '15 Jun 2024, 12:30']]);
  });

  it('shows a date once, naming every tag that carries it', () => {
    expect(rows({ taken: noon, digitized: noon + 60, edited: noon, fileCreatedMs: noon * 1000 })).toEqual([
      ['Taken, edited', '15 Jun 2024, 12:30'],
      ['Digitized', '15 Jun 2024, 12:31'],
      ['File created, modified', '15 Jun 2024, 12:30'],
    ]);
  });

  it("reads the camera's dates as its wall clock and the file's in the machine's zone", () => {
    // The camera's 12:30 is 12:30 wherever the photo is viewed; the file's instant is
    // 14:30 in Berlin in June. Equal as numbers, they are different times, so never merged.
    expect(rows({ taken: noon }, 'Europe/Berlin')).toEqual([
      ['Taken', '15 Jun 2024, 12:30'],
      ['File modified', '15 Jun 2024, 14:30'],
    ]);
  });
});

describe('formatCoordinates', () => {
  it('writes the hemisphere as a letter, not a sign', () => {
    expect(formatCoordinates(48.1374, 11.5755)).toBe('48.13740° N, 11.57550° E');
    expect(formatCoordinates(-33.86882, -151.2093)).toBe('33.86882° S, 151.20930° W');
  });

  it('does not call a rounded zero south or west', () => {
    expect(formatCoordinates(-0.000001, -0.000001)).toBe('0.00000° N, 0.00000° E');
    expect(formatCoordinates(-0.00001, 0)).toBe('0.00001° S, 0.00000° E');
  });
});

describe('nearQuery', () => {
  it('is a near: term the search grammar reads, signs kept', () => {
    expect(nearQuery(48.13742, 11.57549)).toBe('near:48.1374,11.5755');
    expect(nearQuery(-33.86882, 151.2093)).toBe('near:-33.8688,151.2093');
  });
});
