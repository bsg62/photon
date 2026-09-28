import { describe, expect, it } from 'vitest';
import { buildRows, columnsFor, defersThumbs, edgeScrollSpeed, fetchSpan, firstVisibleOffset, GAP, HEADER, itemSpan, itemsInRect, LEAD_MS, LEAD_OVERSCAN_MAX, RENDER_OVERSCAN, renderOverscan, renderRange, rowIndexAt, rowOfItem, tileRow, TILE_WIDTH, topFolderId, totalHeight, TRAIL_OVERSCAN, visibleRange } from './layout';
import { type Motion, STILL } from './scroll-speed.svelte';
import { TILE_SETTLE_MS } from './thumb-request.svelte';

const sections = [
  { folderId: 1, offset: 0, count: 5 },
  { folderId: 2, offset: 5, count: 3 },
];

/** The same runs drawn without headers, as a flat view's run is: geometry tests that want
 *  tile rows only. */
const headerless = <T extends { folderId: number | null }>(runs: T[]): T[] => runs.map((s) => ({ ...s, folderId: null }));

describe('layout', () => {
  it('fits columns to the width', () => {
    expect(columnsFor(800, TILE_WIDTH.medium)).toBe(4);
    expect(columnsFor(100, TILE_WIDTH.medium)).toBe(1);
    expect(columnsFor(0, TILE_WIDTH.medium)).toBe(1);
  });

  it('builds header and tile rows per section', () => {
    const rows = buildRows(sections, 2, TILE_WIDTH.medium);
    expect(rows.map((r) => [r.kind, r.first, r.count, r.top])).toEqual([
      ['header', 0, 0, 0],
      ['tiles', 0, 2, 32],
      ['tiles', 2, 2, 200],
      ['tiles', 4, 1, 368],
      // SECTION_GAP (24) above every header but the first: folder 1 ends at 536.
      ['header', 5, 0, 560],
      ['tiles', 5, 2, 592],
      ['tiles', 7, 1, 760],
    ]);
    expect(totalHeight(rows)).toBe(928);
    expect(totalHeight([])).toBe(0);
  });

  it('hit-tests rows by y', () => {
    const rows = buildRows(sections, 2, TILE_WIDTH.medium);
    expect(rowIndexAt(rows, -5)).toBe(0);
    expect(rowIndexAt(rows, 31)).toBe(0);
    expect(rowIndexAt(rows, 32)).toBe(1);
    expect(rowIndexAt(rows, 500)).toBe(3);
    expect(rowIndexAt(rows, 10_000)).toBe(6);
  });

  it('finds visible ranges and their items', () => {
    const rows = buildRows(sections, 2, TILE_WIDTH.medium);
    expect(visibleRange(rows, 200, 100, 0)).toEqual([2, 3]);
    expect(visibleRange(rows, 0, 10_000, 0)).toEqual([0, 7]);
    expect(visibleRange([], 0, 100, 0)).toEqual([0, 0]);
    expect(itemSpan(rows.slice(0, 3))).toEqual([0, 4]);
    expect(itemSpan(rows.slice(4, 5))).toBeNull();
  });

  it('mounts half a viewport past the edges but fetches two viewports', () => {
    // One flat run of 1000 rows, 168px each: a 1680px viewport is ten rows, so half a
    // viewport is five rows either side and two viewports twenty.
    const rows = buildRows([{ folderId: null, offset: 0, count: 1000 }], 1, TILE_WIDTH.medium);
    const row = tileRow(TILE_WIDTH.medium);
    const viewport = 10 * row;
    const top = 100 * row;

    expect(renderRange(rows, top, viewport, STILL)).toEqual([95, 116]);
    expect(fetchSpan(rows, top, viewport)).toEqual([80, 131]);
  });

  describe('while the grid moves', () => {
    // The same 1000 rows of 168px; a viewport is ten rows.
    const rows = buildRows([{ folderId: null, offset: 0, count: 1000 }], 1, TILE_WIDTH.medium);
    const row = tileRow(TILE_WIDTH.medium);
    const viewport = 10 * row;
    const top = 100 * row;
    const scroll = (direction: 1 | -1, speed: number, peak = speed): Motion => ({ kind: 'scroll', direction, speed, peak });

    it('mounts only what is on screen for a jump', () => {
      expect(renderRange(rows, top, viewport, { kind: 'jump', stream: true })).toEqual([100, 111]);
      expect(renderRange(rows, top, viewport, { kind: 'jump', stream: false })).toEqual([100, 111]);
      // The fetch does not narrow with it: pages are what the tiles show once it stops.
      expect(fetchSpan(rows, top, viewport)).toEqual([80, 131]);
      expect(fetchSpan([], 0, viewport)).toBeNull();
    });

    it('mounts a lead ahead of a fast scroll, and only a little behind it', () => {
      // 4px/ms - a trackpad flick, which used to drop the overscan altogether - leads by
      // LEAD_MS of travel (1200px, seven rows and a bit) and trails by a quarter viewport.
      expect(renderOverscan(scroll(1, 4), viewport)).toEqual([viewport * TRAIL_OVERSCAN, 4 * LEAD_MS]);
      expect(renderRange(rows, top, viewport, scroll(1, 4))).toEqual([97, 118]);
      // Upward, the lead is above.
      expect(renderRange(rows, top, viewport, scroll(-1, 4))).toEqual([92, 113]);
    });

    it('leads by at least the still overscan, and at most LEAD_OVERSCAN_MAX viewports', () => {
      expect(renderOverscan(scroll(1, 0), viewport)[1]).toBe(viewport * RENDER_OVERSCAN);
      expect(renderOverscan(scroll(1, 0.1), viewport)[1]).toBe(viewport * RENDER_OVERSCAN);
      expect(renderOverscan(scroll(1, 100), viewport)[1]).toBe(viewport * LEAD_OVERSCAN_MAX);
    });

    it('sizes the lead by the peak, so a flick slowing down keeps what it mounted', () => {
      expect(renderRange(rows, top, viewport, scroll(1, 0.5, 4))).toEqual(renderRange(rows, top, viewport, scroll(1, 4)));
    });

    it('defers thumbnails only where a tile will be gone before it settles', () => {
      expect(defersThumbs(STILL, viewport)).toBe(false);
      // End or a folder click lands where the user stops.
      expect(defersThumbs({ kind: 'jump', stream: false }, viewport)).toBe(false);
      // A scrollbar drag replaces every tile on the next frame.
      expect(defersThumbs({ kind: 'jump', stream: true }, viewport)).toBe(true);
      // A 4px/ms flick passes a tile through the window in over half a second.
      expect(defersThumbs(scroll(1, 4), viewport)).toBe(false);
      // The window is 2.75 viewports at full lead: 4620px, crossed within the settle above
      // 46.2px/ms.
      const window = viewport * (1 + LEAD_OVERSCAN_MAX + TRAIL_OVERSCAN);
      expect(defersThumbs(scroll(1, window / TILE_SETTLE_MS - 0.1), viewport)).toBe(false);
      expect(defersThumbs(scroll(1, window / TILE_SETTLE_MS + 0.1), viewport)).toBe(true);
    });
  });

  it('draws a run that names no folder without a header', () => {
    // A flat view's (Recent's) one run spans many folders, so there is no folder to name.
    const rows = buildRows([{ folderId: null, offset: 0, count: 5 }], 2, TILE_WIDTH.medium);
    expect(rows.map((r) => [r.kind, r.first, r.count, r.top])).toEqual([
      ['tiles', 0, 2, 0],
      ['tiles', 2, 2, 168],
      ['tiles', 4, 1, 336],
    ]);
    expect(totalHeight(rows)).toBe(504);
  });

  it('names no folder at the top of a flat view', () => {
    // Nothing to remember for the next launch - and the view is not All, where that is kept.
    const rows = buildRows([{ folderId: null, offset: 0, count: 5 }], 2, TILE_WIDTH.medium);
    expect(topFolderId(rows, [{ folderId: null, offset: 0, count: 5 }], 0)).toBeNull();
  });

  it('names the folder at the top of the viewport', () => {
    // What gets remembered for the next launch: whichever folder the eye is on, whether
    // the user scrolled there or clicked it in the sidebar.
    const rows = buildRows(sections, 2, TILE_WIDTH.medium);
    expect(topFolderId(rows, sections, 0)).toBe(1);
    // Still inside folder 1's last tile row.
    expect(topFolderId(rows, sections, 400)).toBe(1);
    // The gap between the two folders still belongs to folder 1: nothing of 2 is up yet.
    expect(topFolderId(rows, sections, 545)).toBe(1);
    // Folder 2's header is at 560.
    expect(topFolderId(rows, sections, 560)).toBe(2);
    expect(topFolderId(rows, sections, 10_000)).toBe(2);
  });

  it('names no folder when there is no grid to be scrolled', () => {
    // A launch before the first scan has produced anything: nothing to remember, and
    // nothing that should overwrite what the last session remembered.
    expect(topFolderId([], [], 0)).toBeNull();
    expect(topFolderId(buildRows(sections, 2, TILE_WIDTH.medium), [], 0)).toBeNull();
  });

  it('locates the row holding an item', () => {
    const rows = buildRows(sections, 2, TILE_WIDTH.medium);
    expect(rowOfItem(rows, 0)).toBe(1);
    expect(rowOfItem(rows, 4)).toBe(3);
    expect(rowOfItem(rows, 5)).toBe(5);
    expect(rowOfItem(rows, 7)).toBe(6);
    expect(rowOfItem(rows, 8)).toBe(-1);
  });
});

describe('itemsInRect', () => {
  // Two sections of 5 photos, 3 columns, no headers: rows at 0 and 168 (tileRow(TILE_WIDTH.medium)).
  const rows = buildRows(headerless([{ folderId: 1, offset: 0, count: 5 }]), 3, TILE_WIDTH.medium);
  const rect = (x0: number, y0: number, x1: number, y1: number) => ({ x0, y0, x1, y1 });

  /** The point of returning ranges rather than one span: a band narrower than the grid
   *  takes a few photos from each row it crosses, and merging those into one range would
   *  select everything between them - the over-selection ids-not-offsets exists to stop. */
  it('keeps a narrow band down one column as one range per row', () => {
    const tall = buildRows(headerless([{ folderId: 1, offset: 0, count: 9 }]), 3, TILE_WIDTH.medium);
    const column2 = GAP + 2 * tileRow(TILE_WIDTH.medium);
    expect(
      itemsInRect(
        tall,
        rect(column2 + 1, 0, column2 + TILE_WIDTH.medium - 1, 3 * tileRow(TILE_WIDTH.medium)),
        TILE_WIDTH.medium,
      ),
    ).toEqual([
      [2, 2],
      [5, 5],
      [8, 8],
    ]);
  });

  /** The left edge is measured from each tile's right edge, so a band starting in the gap
   *  after a tile does not take it. Measuring from the left edges instead agrees everywhere
   *  except here, which is why this case exists. */
  it('does not take the tile to the left of a band that starts in the gap', () => {
    const gapAfterTile0 = GAP + TILE_WIDTH.medium + 2;
    expect(itemsInRect(rows, rect(gapAfterTile0, 10, 1000, 20), TILE_WIDTH.medium)).toEqual([[1, 2]]);
  });

  it('takes the tiles a rectangle touches in one row', () => {
    // Tile k spans x = GAP + k*tileRow(TILE_WIDTH.medium) .. + TILE_WIDTH.medium, so tile 1
    // starts at 176.
    expect(itemsInRect(rows, rect(180, 10, 200, 20), TILE_WIDTH.medium)).toEqual([[1, 1]]);
  });

  it('merges rows that join up into one range', () => {
    expect(itemsInRect(rows, rect(0, 0, 1000, 1000), TILE_WIDTH.medium)).toEqual([[0, 4]]);
  });

  /** The gap below a row belongs to no tile: a band that only grazes it selects nothing,
   *  or dragging between two rows would sweep up both. */
  it('selects nothing from a band inside the gap between rows', () => {
    expect(
      itemsInRect(rows, rect(0, TILE_WIDTH.medium + 1, 1000, tileRow(TILE_WIDTH.medium) - 1), TILE_WIDTH.medium),
    ).toEqual([]);
  });

  it('stops at the last tile of a short row', () => {
    // The second row holds 2 of the 5 photos; a band across it cannot reach a third.
    expect(
      itemsInRect(
        rows,
        rect(0, tileRow(TILE_WIDTH.medium) + 1, 1000, tileRow(TILE_WIDTH.medium) + TILE_WIDTH.medium),
        TILE_WIDTH.medium,
      ),
    ).toEqual([[3, 4]]);
  });

  it('ignores headers', () => {
    const withHeaders = buildRows([{ folderId: 1, offset: 0, count: 2 }], 3, TILE_WIDTH.medium);
    expect(itemsInRect(withHeaders, rect(0, 0, 1000, HEADER - 1), TILE_WIDTH.medium)).toEqual([]);
    expect(itemsInRect(withHeaders, rect(0, 0, 1000, HEADER + TILE_WIDTH.medium), TILE_WIDTH.medium)).toEqual([[0, 1]]);
  });

  it('takes nothing from an empty rectangle or an empty grid', () => {
    expect(itemsInRect(rows, rect(10, 10, 10, 10), TILE_WIDTH.medium)).toEqual([[0, 0]]);
    expect(itemsInRect([], rect(0, 0, 1000, 1000), TILE_WIDTH.medium)).toEqual([]);
  });

  /** Two sections are two runs of rows; a band over both is two ranges only if the offsets
   *  do not run on. Here they do, so it is one. */
  it('joins ranges across sections when the offsets are contiguous', () => {
    const two = buildRows(headerless([
        { folderId: 1, offset: 0, count: 3 },
        { folderId: 2, offset: 3, count: 3 },
      ]), 3, TILE_WIDTH.medium,
    );
    expect(itemsInRect(two, rect(0, 0, 1000, 1000), TILE_WIDTH.medium)).toEqual([[0, 5]]);
  });
});

describe('tile size', () => {
  it('columns depend on the tile size', () => {
    // 800px of canvas: 168px rows at medium, 128 at small, 232 at large.
    expect(columnsFor(800, TILE_WIDTH.medium)).toBe(4);
    expect(columnsFor(800, TILE_WIDTH.small)).toBe(6);
    expect(columnsFor(800, TILE_WIDTH.large)).toBe(3);
  });

  it('rows are taller at a larger tile size', () => {
    const sections = [{ folderId: 1, offset: 0, count: 4 }];
    const small = buildRows(headerless(sections), 2, TILE_WIDTH.small);
    const large = buildRows(headerless(sections), 2, TILE_WIDTH.large);
    expect(small[1].top).toBe(tileRow(TILE_WIDTH.small));
    expect(large[1].top).toBe(tileRow(TILE_WIDTH.large));
  });
});

describe('itemsInRect at other tile sizes', () => {
  // The same shape as the medium-size block above: one section of 5 photos, 3 columns,
  // no headers - re-run at both ends, because a band rule pinned only at 160 is pinned
  // by the one size where a hardcoded 160 would still be right.
  for (const size of ['small', 'large'] as const) {
    const tile = TILE_WIDTH[size];
    const row = tileRow(tile);
    const rows = buildRows(headerless([{ folderId: 1, offset: 0, count: 5 }]), 3, tile);
    const rect = (x0: number, y0: number, x1: number, y1: number) => ({ x0, y0, x1, y1 });

    it(`${size}: a band over a whole row takes the row`, () => {
      expect(itemsInRect(rows, rect(0, 0, 1000, 1000), tile)).toEqual([[0, 4]]);
    });

    it(`${size}: a band starting in the gap after tile 0 starts at tile 1`, () => {
      expect(itemsInRect(rows, rect(GAP + tile + 2, 10, 1000, 20), tile)).toEqual([[1, 2]]);
    });

    it(`${size}: a band wholly inside the gap below a row selects nothing`, () => {
      expect(itemsInRect(rows, rect(0, tile + 1, 1000, row - 1), tile)).toEqual([]);
    });

    it(`${size}: a band over one column of two rows merges into one range`, () => {
      const tall = buildRows(headerless([{ folderId: 1, offset: 0, count: 9 }]), 3, tile);
      // 2 * row lands exactly on row 2's own top edge, and touching counts on the vertical
      // axis (as it does on the horizontal one), so a bottom edge placed there would also
      // take row 2 - not what this case means to show. Backing off by 1px keeps the band
      // inside the gap after row 1, so it covers exactly the two rows the name promises.
      expect(itemsInRect(tall, rect(0, 0, 1000, 2 * row - 1), tile)).toEqual([[0, 5]]);
    });

    it(`${size}: the last short row does not run into the next section`, () => {
      expect(itemsInRect(rows, rect(0, row + 1, 1000, row + tile), tile)).toEqual([[3, 4]]);
    });
  }
});

describe('itemsInRect touching a row edge', () => {
  // Touching counts on the vertical axis as well as the horizontal one: a zero-height
  // band - a click - on a tile's own top or bottom edge is on the tile. The first row's
  // top edge is y=0, so a rule that excluded a touching edge would drop a click there.
  it.each(['small', 'medium', 'large'] as const)('%s: a click on a row edge selects that row', (size) => {
    const tile = TILE_WIDTH[size];
    const rows = buildRows(headerless([{ folderId: 1, offset: 0, count: 3 }]), 3, tile);
    expect(itemsInRect(rows, { x0: 10, y0: 0, x1: 10, y1: 0 }, tile)).toEqual([[0, 0]]);
    expect(itemsInRect(rows, { x0: 10, y0: tile, x1: 10, y1: tile }, tile)).toEqual([[0, 0]]);
  });
});

/** `itemsInRect` walks only the rows between the one holding the band's top and the first
 *  starting below its bottom. These pin the ends of that walk: each case puts an edge where
 *  starting a row late, or stopping a row early, would drop photos the band touches. They
 *  pin behaviour, not speed. */
describe('itemsInRect over part of a grid with headers', () => {
  const tile = TILE_WIDTH.medium; // rows of 168
  // Rows: header 0, tiles [0-2] at 32, [3-4] at 200 (tiles to 360); SECTION_GAP; header
  // 392, tiles [5-7] at 424, [8] at 592 (tiles to 752).
  const rows = buildRows(
    [
      { folderId: 1, offset: 0, count: 5 },
      { folderId: 2, offset: 5, count: 4 },
    ],
    3,
    tile,
  );
  const band = (y0: number, y1: number) => itemsInRect(rows, { x0: 0, y0, x1: 1000, y1 }, tile);

  it('lays out as the cases below assume', () => {
    expect(rows.map((r) => [r.kind, r.first, r.top])).toEqual([
      ['header', 0, 0],
      ['tiles', 0, 32],
      ['tiles', 3, 200],
      ['header', 5, 392],
      ['tiles', 5, 424],
      ['tiles', 8, 592],
    ]);
  });

  it('takes the row a band starts inside, deep in the grid', () => {
    expect(band(250, 260)).toEqual([[3, 4]]);
    expect(band(600, 610)).toEqual([[8, 8]]);
  });

  it('takes the row whose bottom tile edge the band starts on', () => {
    expect(band(360, 360)).toEqual([[3, 4]]);
    // Within a section the next row starts one GAP below that edge, with nothing between.
    expect(band(32 + tile, 32 + tile)).toEqual([[0, 2]]);
    expect(band(200 + tile, 430)).toEqual([[3, 7]]);
  });

  it('takes the row whose top edge the band ends on', () => {
    expect(band(250, 424)).toEqual([[3, 7]]);
    expect(band(100, 200)).toEqual([[0, 4]]);
  });

  it('starts from a header, the section gap or a row gap', () => {
    expect(band(400, 430)).toEqual([[5, 7]]); // top in folder 2's header
    expect(band(380, 430)).toEqual([[5, 7]]); // top in the SECTION_GAP above it
    expect(band(362, 430)).toEqual([[5, 7]]); // top in the gap under row [3-4]
    expect(band(10, 40)).toEqual([[0, 2]]); // top in the first header
  });

  it('reaches past either end of the grid', () => {
    expect(band(-100, 40)).toEqual([[0, 2]]);
    expect(band(600, 100_000)).toEqual([[8, 8]]);
    expect(band(-100, 100_000)).toEqual([[0, 8]]);
    expect(band(5000, 6000)).toEqual([]);
    expect(band(-500, -100)).toEqual([]);
  });

  /** The range-merge rule deep in the grid: a narrow band keeps one range per row. */
  it('keeps a narrow band that starts mid-grid as one range per row', () => {
    const column1 = GAP + tileRow(tile);
    expect(itemsInRect(rows, { x0: column1 + 1, y0: 250, x1: column1 + 2, y1: 600 }, tile)).toEqual([
      [4, 4],
      [6, 6],
    ]);
  });

  /** Every pair of edges from a row boundary and one pixel either side, against a walk of
   *  every row: the bounded walk may skip rows only when they could not have answered. */
  it('agrees with a walk over every row for every edge', () => {
    const everyRow = (y0: number, y1: number): [number, number][] => {
      const ranges: [number, number][] = [];
      for (const row of rows) {
        if (row.kind !== 'tiles' || row.top + tile < y0 || row.top > y1) continue;
        const previous = ranges[ranges.length - 1];
        const to = row.first + row.count - 1;
        if (previous && previous[1] + 1 === row.first) previous[1] = to;
        else ranges.push([row.first, to]);
      }
      return ranges;
    };
    const edges = [-1, ...rows.flatMap((r) => [r.top, r.top + tile, r.top + r.height])];
    const ys = [...new Set(edges.flatMap((y) => [y - 1, y, y + 1]))];
    for (const y0 of ys) {
      for (const y1 of ys) {
        if (y1 < y0) continue;
        expect(band(y0, y1), `${y0}..${y1}`).toEqual(everyRow(y0, y1));
      }
    }
  });
});

describe('keeping your place across a size change', () => {
  const sections = [{ folderId: 1, offset: 0, count: 9 }];

  it('reports the first item of the row at the top of the viewport', () => {
    const rows = buildRows(headerless(sections), 3, TILE_WIDTH.medium);
    expect(firstVisibleOffset(rows, 0)).toBe(0);
    expect(firstVisibleOffset(rows, tileRow(TILE_WIDTH.medium))).toBe(3);
    expect(firstVisibleOffset([], 0)).toBeNull();
  });

  // The pin `Grid.svelte` takes before a size change: what matters is that it names a
  // photo and not a pixel, so it survives every row's `top` moving underneath it. Anywhere
  // within a row answers with that row's first offset, which is what makes the number
  // meaningful in a layout it was not measured in.
  it('names the photo, not the pixel, anywhere within a row', () => {
    const medium = buildRows(headerless(sections), 3, TILE_WIDTH.medium);
    const row = tileRow(TILE_WIDTH.medium);
    expect(firstVisibleOffset(medium, 2 * row)).toBe(6);
    expect(firstVisibleOffset(medium, 2 * row + row - 1)).toBe(6);
  });

  // Two folders, so there is a header partway down to land on.
  const folders = [
    { folderId: 1, offset: 0, count: 5 },
    { folderId: 2, offset: 5, count: 4 },
  ];

  // The claim the whole feature rests on, and the one an assertion made inside a single
  // layout cannot reach: the pin is read from the layout the user was looking at and spent
  // in the one that replaces it, so it has to name a row *there*. Medium at three columns
  // and small at five share no row tops at all, which is the point.
  it('finds its row in the layout the pin was not taken in', () => {
    const medium = buildRows(folders, 3, TILE_WIDTH.medium);
    const small = buildRows(folders, 5, TILE_WIDTH.small);
    for (const top of [0, 40, 200, 500, totalHeight(medium) - 1]) {
      const offset = firstVisibleOffset(medium, top);
      expect(offset).not.toBeNull();
      expect(rowOfItem(small, offset!)).toBeGreaterThanOrEqual(0);
    }
  });

  // A header is where the pin can be wrong without being out of range, so this is the case
  // that discriminates: at a section's header the eye is on that section's first photo, and
  // an implementation that answered with the tile row above - the last row of the previous
  // folder - would scroll the user back into a folder they had already left. It round-trips
  // because `scrollToOffset(offset, 'start')` puts the header itself back at the top.
  it('answers a header with the section it heads, not the row above it', () => {
    const medium = buildRows(folders, 3, TILE_WIDTH.medium);
    const header = medium.find((r) => r.kind === 'header' && r.section === 1)!;
    expect(firstVisibleOffset(medium, header.top)).toBe(5);
    expect(rowOfItem(buildRows(folders, 5, TILE_WIDTH.small), 5)).toBeGreaterThanOrEqual(0);
  });
});

describe('edgeScrollSpeed', () => {
  // A viewport 500px tall on screen at y = 100..600, with a 40px margin.
  const speed = (y: number) => edgeScrollSpeed(y, 100, 600, 40, 20);

  it('does not scroll while the pointer is well inside the viewport', () => {
    expect(speed(300)).toBe(0);
    expect(speed(141)).toBe(0);
    expect(speed(559)).toBe(0);
  });

  /** The boundaries themselves. Whether the comparison is `<` or `<=` cannot matter - the
   *  ramp is zero at the boundary either way - so this pins the behaviour (no movement, and
   *  movement one pixel further out) rather than the operator. `Math.abs` because the top
   *  edge computes `-0`, which `toBe(0)` refuses. */
  it('holds still exactly at the edge of the margin', () => {
    expect(Math.abs(speed(140))).toBe(0);
    expect(Math.abs(speed(560))).toBe(0);
    expect(speed(139)).toBeLessThan(0);
    expect(speed(561)).toBeGreaterThan(0);
  });

  /** A viewport with no height has no margins to be inside. Without the guard the ramp
   *  divides by zero and answers ±max for every point, so a grid measured before its first
   *  layout would scroll at full speed. */
  it('never scrolls a viewport with no height, or with no margin', () => {
    expect(edgeScrollSpeed(100, 100, 100, 40, 20)).toBe(0);
    expect(edgeScrollSpeed(120, 100, 100, 40, 20)).toBe(0);
    expect(edgeScrollSpeed(300, 100, 600, 0, 20)).toBe(0);
    expect(edgeScrollSpeed(110, 100, 600, 0, 20)).toBe(0);
  });

  it('scrolls faster the deeper into the margin the pointer is', () => {
    const shallow = speed(590);
    const deep = speed(599);
    expect(shallow).toBeGreaterThan(0);
    expect(deep).toBeGreaterThan(shallow);
  });

  it('scrolls up at the top edge and down at the bottom', () => {
    expect(speed(110)).toBeLessThan(0);
    expect(speed(590)).toBeGreaterThan(0);
  });

  /** A pointer dragged off the window entirely must not scroll arbitrarily fast: the drag
   *  is captured, so this is the common case, not an edge one. */
  it('clamps to the maximum however far outside the viewport the pointer goes', () => {
    expect(speed(601)).toBe(20);
    expect(speed(5000)).toBe(20);
    expect(speed(99)).toBe(-20);
    expect(speed(-5000)).toBe(-20);
  });

  /** A viewport shorter than two margins would otherwise be all edge, and a band in a short
   *  grid would scroll wherever the pointer rested. 100..160 is 60 tall, so each margin is
   *  capped at 20 and the middle third holds still. */
  it('keeps a still middle third in a viewport shorter than its margins', () => {
    expect(edgeScrollSpeed(130, 100, 160, 40, 20)).toBe(0);
    expect(edgeScrollSpeed(105, 100, 160, 40, 20)).toBeLessThan(0);
    expect(edgeScrollSpeed(155, 100, 160, 40, 20)).toBeGreaterThan(0);
  });
});
