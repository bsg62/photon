# The website in photon's own dress

2026-10-08. Direction approved in conversation the same day, from a full-page mock.

## What it is

A restyle of `site/index.html` and `site/404.html` so the page looks like the app it describes:
photon's real tokens, its icons, its key caps and a tighter type scale. The layout, the sections,
their order and every sentence stay as they are after the content update of the same day
(`b14b247`).

Three bolder directions were drawn first (a full-bleed photograph, a magazine page, a contact
sheet) and turned down as too bold for the project. Of three steps towards "look like the app",
the quietest was chosen: no top bar, no sidebar of sections, no bordered cards, no Light/Dark
control. This spec is that step and nothing from the other two.

## What does not change

- One hand-written page, inline CSS, no JavaScript, no build step. `pages.yml` still uploads
  `site/` as it is.
- No request to any other host: no web font, no analytics, no icon CDN. The page says photon does
  not connect to the internet, and it should not either.
- The wording, with one exception: keys that are today written as code or plain text become
  `<kbd>` elements (below). No sentence is added, removed or reordered.
- Light and dark follow `prefers-color-scheme`. There is no switch.
- `og.jpg`, the icon, `CNAME`, the download and repository links.

## Decisions

- **The tokens are the app's, value for value.** Today the page approximates two of them
  (`--line` as a solid grey, `--chrome` for code backgrounds). It takes `--surface`, `--field`,
  `--line`, `--text`, `--text-dim`, `--accent` and `--on-accent` from `ui/src/tokens.css`, in both
  themes. `--chrome` is no longer used and goes. One value is the page's own: `--shadow`, the
  screenshot's shadow (`0 6px 24px #00000022` light, `#00000066` dark).
- **The primary button's text is `--on-accent`,** not `--surface`: white on the light theme's
  blue, `#111111` on the dark theme's lighter blue, the pair `tokens.test.ts` already holds to
  4.5:1. The secondary button is `--field` with `--text`, as a control in the app is.
- **The type scale**, system font as today:

  | Element | Today | New |
  |---|---|---|
  | Body | 17px / 1.6 | 16px / 1.6 |
  | `h1` | 44px | 40px, weight 700, line-height 1.1, tracking -0.02em |
  | Tagline | 20px | 18px |
  | `h2` | 24px | 22px, weight 650, line-height 1.25, tracking -0.01em |
  | Feature heading | 16.8px | 15px, weight 600 |
  | Feature text | 17px | 14.5px / 1.55 |
  | FAQ heading | 16.8px | 16px, weight 600 |
  | Table | 17px | 14.5px; header cells 12px, weight 600, `--text-dim` |
  | Caption, platforms line, footer | 15.2px | 13px |

- **Prose has a measure.** `main` goes from 60rem to 56rem with 1.5rem of side padding (1rem on a
  phone). Paragraphs and lists directly in `main`, and the FAQ's paragraphs, are held to 44rem;
  the screenshots, the feature grid and the tables use the full width. Today a paragraph runs to
  about 120 characters a line.
- **Each feature has one of the app's icons** before its heading, 16px, stroked in `--accent`,
  decoration only (`aria-hidden`). The nine, by heading: Picasa `star`, big libraries
  `layout-grid`, reorganise `folder`, search `search`, albums and keywords `tag`, people `user`,
  duplicates `copy`, hides `eye-off`, rotate/crop/export `crop`. The path data is copied from
  `ui/src/lib/icons.ts` into one hidden `<svg>` of `<symbol>`s at the top of `<body>`, and each
  heading references its symbol with `<use>`. No other heading gets an icon.
- **The icons' licence travels with them.** The page is a distribution of its own, so an HTML
  comment above the symbols carries what `icons.ts` carries: Lucide, ISC, its copyright line, and
  that `search` derives from Feather (MIT, its copyright line), with a pointer to
  `THIRD-PARTY-NOTICES.md` in the repository. The footer's credit line gains "Icons from Lucide",
  linked to that file. This is the one visible addition to the page's text.
- **A key is a key cap.** `<kbd>` is drawn as `ShortcutList.svelte` draws one: `--field` ground,
  an inset 1px `--line`, 6px radius, the text's own font at 0.8125em. These become `<kbd>`: `H` in
  the hide card; in the Also list `+`, `-`, `Ctrl` (the wheel), `Ctrl`+`C` and `⌘` `C`, `?`, `.`,
  `H`, `Shift`, and `Ctrl`+`B`. Not the `?` in the search card: that one is the button in the
  search box, not a key, and stays code. Search terms, file names and paths stay `<code>`.
- **Code chips** are `--field`, 4px radius, a monospace stack at 0.8125em (13px in body text).
- **Screenshots** have an 8px radius (10px today), a 1px `--line` hairline and `--shadow`, drawn
  as one `box-shadow` so the hairline is not a border that changes the image's box.
- **Focus** is the app's ring: `2px solid var(--accent)`, offset 2px, on `:focus-visible`.
- **List markers** are `--text-dim`.
- **`404.html`** takes the same tokens, font size, `h1` weight and tracking, and focus ring. Its
  layout and text stay.
- **The screenshots are regenerated.** The dark main-window screenshot that is live shows a blank
  third tile where the light one shows the Lisbon photo. `main-light`, `main-dark` and
  `viewer-info-light` are made again with `cargo run -p xtask -- screenshots --photos
  crates/xtask/screenshots/photos` and converted as CLAUDE.md describes, `og.jpg` included. Why
  the tile was blank is not known yet. If it comes back blank, the cause is found before
  anything is published: a shot taken before its thumbnails had loaded is a defect in the
  harness, fixed there with a test, and not papered over by retrying until it happens to load.
- **The page's tokens are held to the app's by a test.** The stylesheet's opening comment says
  the palette is the app's "so the page and the screenshots agree", and nothing checked it: the
  two approximations above are how far it drifted. A vitest test (`ui/src/lib/site.test.ts`,
  plain file reads, as `search-help.test.ts` reads `search.rs`) parses the light and dark blocks of `site/index.html` and
  `site/404.html` and fails when a token either declares differs from the same token in
  `tokens.css`, or is one `tokens.css` does not have. `--shadow` is the named exception. It is
  shown to fail with one value changed in the page, per the convention.

## For review

Two things in this spec go beyond what the mock showed, and either can be struck without
touching the rest:

1. The "Icons from Lucide" words in the footer. The comment in the source is the licence notice;
   the visible credit is a courtesy.
2. The token test. Without it the design is the same and the comment stays a promise.

## Verification

There is no harness that renders the site. What is checked:

- `npm run check` and `npm test` (the new test among them).
- The page in headless Chromium at 1280px and 390px wide, light and with `--force-dark-mode`
  (which sets `prefers-color-scheme: dark` in headless): the nine features in three rows of
  three on the desktop and one column on the phone, no horizontal overflow at either width,
  every icon drawn, the dark Download button's text dark.
- `404.html` the same way, both themes.
- The three regenerated screenshots looked at before they are converted: no blank tile.

It is Chromium's rendering only. Safari and Firefox are for a person to look at; the page uses
nothing newer than CSS custom properties, `<picture>` and inline SVG `<use>`.

## Not in this

A top bar or any navigation, bordered feature cards, a Light/Dark control, more screenshots, a
second page, new wording, a web font. Each was drawn or discussed and not chosen.
