import { describe, expect, it } from 'vitest';
import tokens from '../tokens.css?raw';
import index from '../../../site/index.html?raw';
import notFound from '../../../site/404.html?raw';

// The website (site/) is hand-written and has no build step, so nothing shares a line of CSS
// with the app. Its stylesheet says its palette is the app's; this file is what holds it to
// that. It had drifted before: two tokens were approximations nobody had compared.

type Tokens = Record<string, string>;

/** The page's tokens that are its own and have no counterpart in tokens.css. */
const OWN = ['--shadow'];

const PAGES: [string, string][] = [
  ['index.html', index],
  ['404.html', notFound],
];

const bare = (css: string) => css.replace(/\/\*[\s\S]*?\*\//g, '');

function props(body: string): Tokens {
  const out: Tokens = {};
  for (const decl of body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) out[decl[1]] = decl[2].trim();
  return out;
}

/** The app's tokens for one theme: every rule of tokens.css whose selector list holds `selector`. */
function appTheme(selector: string): Tokens {
  const out: Tokens = {};
  for (const rule of bare(tokens).matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    const selectors = rule[1].split(',').map((s) => s.trim());
    if (selectors.includes(selector)) Object.assign(out, props(rule[2]));
  }
  return out;
}

/** A page's stylesheet, without its comments. */
function pageCss(html: string): string {
  const style = /<style>([\s\S]*?)<\/style>/.exec(html);
  if (!style) throw new Error('the page has no <style> block');
  return bare(style[1]);
}

/** A page's own tokens: its `:root` block outside any media query, and the one inside
 *  `prefers-color-scheme: dark`. */
function pageThemes(html: string): { light: Tokens; dark: Tokens } {
  const css = pageCss(html);
  const dark = /@media \(prefers-color-scheme: dark\)\s*\{\s*:root\s*\{([^}]*)\}\s*\}/.exec(css);
  if (!dark) throw new Error('the page has no dark :root block');
  const light = /:root\s*\{([^}]*)\}/.exec(css.replace(dark[0], ''));
  if (!light) throw new Error('the page has no light :root block');
  return { light: props(light[1]), dark: props(dark[1]) };
}

describe.each(PAGES)('site/%s', (_name, html) => {
  const page = pageThemes(html);

  it.each([
    ['light', "[data-theme='light']"],
    ['dark', "[data-theme='dark']"],
  ] as const)("declares the app's %s tokens, value for value", (theme, selector) => {
    const app = appTheme(selector);
    const shared = Object.entries(page[theme]).filter(([name]) => !OWN.includes(name));
    expect(shared.length).toBeGreaterThan(0);
    // A token tokens.css does not have compares against undefined, and fails by name.
    for (const [name, value] of shared) expect(value, `${name} (${theme})`).toBe(app[name]);
  });

  it('declares every token for both themes', () => {
    // One missing from the dark block is not an error anywhere: dark shows the light value.
    expect(Object.keys(page.dark).sort()).toEqual(Object.keys(page.light).sort());
  });

  it('reads no token it does not declare', () => {
    // `var(--chrome)` left behind after the token went computes to nothing, silently.
    const read = new Set([...pageCss(html).matchAll(/var\((--[\w-]+)\)/g)].map((m) => m[1]));
    for (const name of read) expect(Object.keys(page.light), name).toContain(name);
  });
});
