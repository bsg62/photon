import { describe, expect, it } from 'vitest';
import css from '../tokens.css?raw';

type Tokens = Record<string, string>;

/** The custom properties of every rule whose selector list contains `selector` exactly. */
function block(source: string, selector: string): Tokens {
  const out: Tokens = {};
  const bare = source.replace(/\/\*[\s\S]*?\*\//g, '');
  for (const rule of bare.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    const selectors = rule[1].split(',').map((s) => s.trim());
    if (!selectors.includes(selector)) continue;
    for (const decl of rule[2].matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) out[decl[1]] = decl[2].trim();
  }
  return out;
}

/** `#rrggbb` or `#rrggbbaa` to [r, g, b, alpha 0..1]. */
function rgba(hex: string): [number, number, number, number] {
  const m = /^#([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(hex);
  if (!m) throw new Error(`not a 6- or 8-digit hex colour: ${hex}`);
  const n = parseInt(m[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255, m[2] ? parseInt(m[2], 16) / 255 : 1];
}

/** A translucent colour as it appears over an opaque ground. */
function over(top: string, ground: string): string {
  const [r, g, b, a] = rgba(top);
  const [R, G, B] = rgba(ground);
  const mix = (f: number, bk: number) => Math.round(f * a + bk * (1 - a));
  return '#' + [mix(r, R), mix(g, G), mix(b, B)].map((v) => v.toString(16).padStart(2, '0')).join('');
}

/** WCAG 2.x relative luminance and contrast ratio. */
function luminance(hex: string): number {
  const [r, g, b] = rgba(hex).slice(0, 3).map((v) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const themes = {
  light: block(css, "[data-theme='light']"),
  dark: block(css, "[data-theme='dark']"),
};

describe('the contrast helpers', () => {
  it('agree with the WCAG reference values', () => {
    expect(contrast('#000000', '#ffffff')).toBeCloseTo(21, 5);
    expect(contrast('#777777', '#ffffff')).toBeCloseTo(4.48, 2);
    expect(over('#00000080', '#ffffff')).toBe('#7f7f7f');
  });
});

describe.each(Object.entries(themes))('the %s theme', (_name, t) => {
  const grounds = ['--surface', '--chrome'] as const;

  it.each(grounds)('has readable text on %s', (ground) => {
    expect(contrast(t['--text'], t[ground])).toBeGreaterThanOrEqual(4.5);
    expect(contrast(t['--text-dim'], t[ground])).toBeGreaterThanOrEqual(4.5);
    expect(contrast(t['--danger'], t[ground])).toBeGreaterThanOrEqual(4.5);
  });

  it.each(grounds)('has an accent that stands out on %s', (ground) => {
    expect(contrast(t['--accent'], t[ground])).toBeGreaterThanOrEqual(3);
  });

  it('has readable text on a primary button', () => {
    expect(contrast(t['--on-accent'], t['--accent'])).toBeGreaterThanOrEqual(4.5);
  });

  it('keeps a sidebar row readable when hovered and when active', () => {
    // A hovered row still shows its dim count; an active row's count switches to --text,
    // because --text-dim does not reach 4.5 over --accent-soft (spec §2, §5).
    expect(contrast(t['--text-dim'], over(t['--hover'], t['--chrome']))).toBeGreaterThanOrEqual(4.5);
    expect(contrast(t['--text'], over(t['--accent-soft'], t['--chrome']))).toBeGreaterThanOrEqual(4.5);
  });
});

describe('the viewer glass', () => {
  it("keeps dim text readable on the viewer's glass over the brightest photo", () => {
    const ground = over(themes.dark['--glass'], '#ffffff');
    expect(contrast(themes.dark['--text-dim'], ground)).toBeGreaterThanOrEqual(4.5);
    expect(contrast(themes.dark['--text'], ground)).toBeGreaterThanOrEqual(4.5);
  });
});

/** The raw (comment-stripped) body of the rule whose selector list contains `selector`
 *  exactly - like `block()`, but keeps every declaration, not only `--*` ones, so a plain
 *  property such as `accent-color` can be asserted on too. `exact: true` additionally
 *  requires `selector` to be the rule's *only* selector, to tell the scales-only `:root`
 *  block apart from `:root, [data-theme]`, which also matches on `:root`. */
function ruleBody(source: string, selector: string, { exact = false } = {}): string {
  const bare = source.replace(/\/\*[\s\S]*?\*\//g, '');
  for (const rule of bare.matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    const selectors = rule[1].split(',').map((s) => s.trim());
    if (exact ? selectors.length === 1 && selectors[0] === selector : selectors.includes(selector)) {
      return rule[2];
    }
  }
  throw new Error(`no rule has the selector ${selector}`);
}

describe('the structure of tokens.css', () => {
  it('places the dark block after the light block, since equal specificity on the root leaves source order as the only tiebreaker', () => {
    const bare = css.replace(/\/\*[\s\S]*?\*\//g, '');
    const lightIndex = bare.indexOf("[data-theme='light']");
    const darkIndex = bare.indexOf("[data-theme='dark']");
    expect(lightIndex).toBeGreaterThanOrEqual(0);
    expect(darkIndex).toBeGreaterThan(lightIndex);
  });

  it('declares accent-color beside the aliases, on [data-theme], not in the theme-independent scales block', () => {
    expect(ruleBody(css, '[data-theme]')).toMatch(/accent-color\s*:\s*var\(--accent\)\s*;/);
    expect(ruleBody(css, ':root', { exact: true })).not.toMatch(/accent-color/);
  });

  it('aliases --bg/--panel/--panel-2/--muted to --surface/--chrome/--raised/--text-dim, declared on [data-theme] too', () => {
    const aliases = block(css, '[data-theme]');
    expect(aliases['--bg']).toBe('var(--surface)');
    expect(aliases['--panel']).toBe('var(--chrome)');
    expect(aliases['--panel-2']).toBe('var(--raised)');
    expect(aliases['--muted']).toBe('var(--text-dim)');
  });
});

describe('the two themes', () => {
  it('define the same tokens, bar the viewer-only glass', () => {
    const glass = ['--glass', '--glass-line'];
    const names = (t: Tokens) => Object.keys(t).filter((n) => !glass.includes(n)).sort();
    expect(names(themes.light).length).toBeGreaterThan(10);
    expect(names(themes.dark)).toEqual(names(themes.light));
    for (const name of glass) expect(themes.dark[name]).toBeDefined();
  });
});
