import { describe, expect, it } from 'vitest';
import appCss from '../app.css?raw';

/** Every component, as text. The reskin's definition of done, and what keeps it done: a
 *  colour belongs in tokens.css, an icon in icons.ts. */
const components = import.meta.glob('../**/*.svelte', { query: '?raw', import: 'default', eager: true }) as Record<
  string,
  string
>;

/** The viewer's ground. Not a theme decision: a photo is judged against black. */
const ALLOWED = [{ file: '../components/Viewer.svelte', literal: '#000' }];

/** Removed with the aliases; an unknown custom property computes to nothing, silently. */
const OLD_NAMES = ['--bg', '--panel', '--panel-2', '--muted'];

const GLYPHS = /[⚙★☆🕘⧉▸▾⚠✕↺↻✂▶⏸ⓘ]/u;

function styleOf(source: string): string {
  return (/<style[^>]*>([\s\S]*?)<\/style>/.exec(source)?.[1] ?? '').replace(/\/\*[\s\S]*?\*\//g, '');
}

/** Markup and script without comments, where a glyph may be mentioned in prose. */
function codeOf(source: string): string {
  return source
    .replace(/<style[^>]*>[\s\S]*?<\/style>/, '')
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/^\s*\/\/.*$/gm, '');
}

function literals(css: string): string[] {
  return css.match(/#[0-9a-fA-F]{3,8}\b|rgba?\([^)]*\)|hsla?\([^)]*\)/g) ?? [];
}

const styled = [...Object.entries(components), ['../app.css', appCss] as const];

describe('component styles', () => {
  it('finds the components', () => {
    expect(Object.keys(components).length).toBeGreaterThan(8);
    expect(styleOf(components['../components/Tile.svelte'])).toContain('.tile');
  });

  it.each(styled)('%s has no colour literal', (file, source) => {
    const css = file.endsWith('.css') ? source.replace(/\/\*[\s\S]*?\*\//g, '') : styleOf(source);
    const found = literals(css).filter((l) => !ALLOWED.some((a) => a.file === file && a.literal === l));
    expect(found).toEqual([]);
  });

  it.each(styled)('%s uses none of the removed variable names', (file, source) => {
    const used = OLD_NAMES.filter((name) => new RegExp(`var\\(\\s*${name}\\s*[,)]`).test(source));
    expect(used).toEqual([]);
  });

  it.each(Object.entries(components))('%s draws no icon with a glyph', (_file, source) => {
    expect(GLYPHS.exec(codeOf(source))?.[0]).toBeUndefined();
  });
});
