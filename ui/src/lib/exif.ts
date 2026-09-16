/** Formatting for the viewer's info panel. Pure, so every spelling is pinned by a test;
 *  the backend stores the raw numbers and derives nothing. */

import type { ViewerItem } from './api';

/** One line of the info panel. */
export interface InfoRow {
  label: string;
  value: string;
}

/** "Canon EOS 5D Mark IV", not "Canon Canon EOS 5D Mark IV": most makers repeat the make
 *  in the model, and Nikon writes "NIKON CORPORATION" as the make and "NIKON D750" as the
 *  model. The model wins whenever it already names the maker's first word. */
export function cameraName(make: string | null, model: string | null): string | null {
  const m = make?.trim() ?? '';
  const d = model?.trim() ?? '';
  if (!d) return m || null;
  if (!m) return d;
  const first = m.split(/\s+/)[0]?.toLowerCase() ?? '';
  if (first && d.toLowerCase().includes(first)) return d;
  return `${m} ${d}`;
}

/** Millimetres to one decimal only when the value has one: "50 mm", "18.5 mm". */
export function formatFocal(mm: number): string {
  const rounded = Math.round(mm * 10) / 10;
  return `${Number.isInteger(rounded) ? rounded.toFixed(0) : rounded.toFixed(1)} mm`;
}

/** "f/1.8", "f/11": one decimal where the camera has one. */
export function formatAperture(f: number): string {
  const rounded = Math.round(f * 10) / 10;
  return `f/${Number.isInteger(rounded) ? rounded.toFixed(0) : rounded.toFixed(1)}`;
}

/** Shutter speeds the way the camera shows them: "1/250 s" below a second, "2 s" or
 *  "2.5 s" above, and "0.5 s" rather than "1/2 s" for the slow fractions where the
 *  reciprocal is not what anyone reads off a dial. */
export function formatExposure(seconds: number): string {
  if (seconds <= 0) return '';
  if (seconds >= 1) {
    const rounded = Math.round(seconds * 10) / 10;
    return `${Number.isInteger(rounded) ? rounded.toFixed(0) : rounded.toFixed(1)} s`;
  }
  const reciprocal = 1 / seconds;
  if (reciprocal >= 4) return `1/${Math.round(reciprocal)} s`;
  return `${(Math.round(seconds * 10) / 10).toFixed(1)} s`;
}

export function formatIso(iso: number): string {
  return `ISO ${iso}`;
}

/** The camera rows of the info panel, in the order a photographer reads them; a field the
 *  camera did not write is simply absent, so a scan with no EXIF gets no rows at all. */
export function cameraRows(item: Pick<ViewerItem, 'make' | 'model' | 'lens' | 'focalMm' | 'aperture' | 'exposureS' | 'iso'>): InfoRow[] {
  const rows: InfoRow[] = [];
  const camera = cameraName(item.make, item.model);
  if (camera) rows.push({ label: 'Camera', value: camera });
  if (item.lens) rows.push({ label: 'Lens', value: item.lens });
  const exposure: string[] = [];
  if (item.focalMm) exposure.push(formatFocal(item.focalMm));
  if (item.aperture) exposure.push(formatAperture(item.aperture));
  if (item.exposureS) exposure.push(formatExposure(item.exposureS));
  if (item.iso) exposure.push(formatIso(item.iso));
  if (exposure.length) rows.push({ label: 'Exposure', value: exposure.join(' · ') });
  return rows;
}
