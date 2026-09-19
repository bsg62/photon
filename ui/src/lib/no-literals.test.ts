import { describe, expect, it } from 'vitest';
import appCss from '../app.css?raw';
import tokensCss from '../tokens.css?raw';

/** Every component, as text. The reskin's definition of done, and what keeps it done: a
 *  colour belongs in tokens.css, an icon in icons.ts. */
const components = import.meta.glob('../**/*.svelte', { query: '?raw', import: 'default', eager: true }) as Record<
  string,
  string
>;

/** The viewer's ground. Not a theme decision: a photo is judged against black. Exact, not a
 *  ceiling: a second literal slipped in beside it would pass a mere "is #000 allowed" check. */
const ALLOWED = [{ file: '../components/Viewer.svelte', literal: '#000' }];

/** Removed with the aliases; an unknown custom property computes to nothing, silently. */
const OLD_NAMES = ['--bg', '--panel', '--panel-2', '--muted'];

/** `--sidebar-width` is set by a `style:` binding in App.svelte, not declared in tokens.css. */
const UNDEFINED_VAR_EXCEPTIONS = ['--sidebar-width'];

/** The glyphs the UI once used as icons, before Icon.svelte. `×` is included because no
 *  component contains one as text: a dimension string like `5472 × 3648` is built in
 *  lib/caption.ts, which this test does not read. */
const GLYPHS = /[⚙★☆🕘⧉▸▾⚠✕×↺↻✂▶⏸ⓘ]/u;

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

/** Hex, rgb/hsl functions, the named greys and the modern colour functions - as values, never
 *  inside an identifier, a custom-property name or `white-space`. `\b` alone lets `--muted`-
 *  free text like `white-space` or `--white` through, so `white`/`black` need a lookahead/behind
 *  that also rejects a leading `-` or a following `-`. */
function literals(css: string): string[] {
  return (
    css.match(
      /#[0-9a-fA-F]{3,8}\b|rgba?\([^)]*\)|hsla?\([^)]*\)|color-mix\(|oklch\(|oklab\(|lab\(|lch\(|(?<![\w-])(?:white|black)(?![\w-])/gi,
    ) ?? []
  );
}

const styled = [...Object.entries(components), ['../app.css', appCss] as const];

/** Every `var(--x)` mentioned, as the bare name. */
function varsUsed(source: string): string[] {
  return [...source.matchAll(/var\(\s*(--[\w-]+)\s*[,)]/g)].map((m) => m[1]);
}

/** Every `--x: ` custom property tokens.css declares, anywhere in the file. */
const declared = new Set([...tokensCss.matchAll(/(--[\w-]+)\s*:/g)].map((m) => m[1]));

describe('component styles', () => {
  it('finds the components', () => {
    expect(Object.keys(components).length).toBeGreaterThan(8);
    expect(styleOf(components['../components/Tile.svelte'])).toContain('.tile');
    // If vitest's raw-import were ever lost, `?raw` would resolve to an empty string and
    // every app.css assertion below would pass vacuously on nothing.
    expect(appCss).toContain('font-family');
  });

  it.each(styled)('%s has no colour literal', (file, source) => {
    const css = file.endsWith('.css') ? source.replace(/\/\*[\s\S]*?\*\//g, '') : styleOf(source);
    const found = literals(css).filter((l) => !ALLOWED.some((a) => a.file === file && a.literal === l));
    expect(found).toEqual([]);
  });

  it("allows the viewer exactly one colour literal, and it is #000", () => {
    const css = styleOf(components['../components/Viewer.svelte']);
    expect(literals(css)).toEqual(['#000']);
  });

  it.each(styled)('%s uses none of the removed variable names', (file, source) => {
    const used = OLD_NAMES.filter((name) => new RegExp(`var\\(\\s*${name}\\s*[,)]`).test(source));
    expect(used).toEqual([]);
  });

  it.each(styled)('%s uses no undefined custom property', (_file, source) => {
    const used = new Set(varsUsed(source));
    const missing = [...used].filter((name) => !declared.has(name) && !UNDEFINED_VAR_EXCEPTIONS.includes(name));
    expect(missing).toEqual([]);
  });

  it.each(Object.entries(components))('%s draws no icon with a glyph', (_file, source) => {
    expect(GLYPHS.exec(codeOf(source))?.[0]).toBeUndefined();
  });
});
