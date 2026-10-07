import { describe, expect, it } from 'vitest';
import { buildRows, columnsFor, layoutHeight, pageMove, placeIn, rowWidth, showsTimeline, TIMELINE_WIDTH, defersThumbs, edgeScrollSpeed, fetchSpan, GAP, hasHeader, HEADER, itemSpan, itemsInRect, headerRows, LEAD_MS, LEAD_OVERSCAN_MAX, pinAt, pinnedHeader, pinTop, RENDER_OVERSCAN, renderOverscan, renderRange, rowIndexAt, rowOfItem, scrollIntoGrid, scrollToStart, SECTION_GAP, TILE_MAX, tileFor, tileRow, TILE_WIDTH, topFolderId, totalHeight, TRAIL_OVERSCAN, visibleRange } from './layout';
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

describe('keeping your place across a change of tile width', () => {
  const sections = [{ folderId: 1, offset: 0, count: 9 }];

  it('reports the first item of the row at the top of the viewport', () => {
    const rows = buildRows(headerless(sections), 3, TILE_WIDTH.medium);
    expect(pinAt(rows, 0)).toEqual({ offset: 0, header: false, into: 0 });
    expect(pinAt(rows, tileRow(TILE_WIDTH.medium))).toEqual({ offset: 3, header: false, into: 0 });
    expect(pinAt([], 0)).toBeNull();
  });

  // The pin `Grid.svelte` takes before the tiles change width: what matters is that it names
  // a photo and not a pixel, so it survives every row's `top` moving underneath it. Anywhere
  // within a row answers with that row's first offset, which is what makes the number
  // meaningful in a layout it was not measured in.
  it('names the photo, not the pixel, anywhere within a row', () => {
    const medium = buildRows(headerless(sections), 3, TILE_WIDTH.medium);
    const row = tileRow(TILE_WIDTH.medium);
    expect(pinAt(medium, 2 * row)?.offset).toBe(6);
    expect(pinAt(medium, 2 * row + row - 1)?.offset).toBe(6);
  });

  // Tiles that fill the row change width with every pixel the window or the sidebar moves,
  // so the pin is spent on every frame of a drag: one that put the row's top at the top of
  // the grid would jump by up to a row on the first frame. The part of the row already
  // scrolled past is kept as a share of its height, which is the same photo detail at the
  // top edge whatever the row's new height.
  it('comes back to the same part of the row, not to its top', () => {
    const medium = buildRows(headerless(sections), 3, TILE_WIDTH.medium);
    const wider = buildRows(headerless(sections), 3, 200);
    const pin = pinAt(medium, tileRow(TILE_WIDTH.medium) + tileRow(TILE_WIDTH.medium) / 4)!;
    expect(pin).toEqual({ offset: 3, header: false, into: 0.25 });
    expect(pinTop(wider, pin)).toBe(tileRow(200) + tileRow(200) / 4);
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
      const pin = pinAt(medium, top);
      expect(pin).not.toBeNull();
      const back = pinTop(small, pin!);
      expect(back).not.toBeNull();
      expect(pinAt(small, back!)?.offset).toBe(rowsFirst(small, pin!.offset));
    }
  });

  /** The first offset of the row of `rows` that holds `offset`. */
  function rowsFirst(rows: ReturnType<typeof buildRows>, offset: number): number {
    return rows[rowOfItem(rows, offset)].first;
  }

  // A header is where the pin can be wrong without being out of range, so this is the case
  // that discriminates: at a section's header the eye is on that section's first photo, and
  // an implementation that answered with the tile row above - the last row of the previous
  // folder - would scroll the user back into a folder they had already left. And it comes
  // back to the header itself, not to the row of photos under it, which shares its offset.
  it('answers a header with the section it heads, and comes back to that header', () => {
    const medium = buildRows(folders, 3, TILE_WIDTH.medium);
    const small = buildRows(folders, 5, TILE_WIDTH.small);
    const header = medium.find((r) => r.kind === 'header' && r.section === 1)!;
    const pin = pinAt(medium, header.top)!;
    expect(pin).toEqual({ offset: 5, header: true, into: 0 });
    expect(pinTop(small, pin)).toBe(small.find((r) => r.kind === 'header' && r.section === 1)!.top);
    // The row under it is another place, a header's height further down.
    expect(pinTop(small, { ...pin, header: false })).toBe(small.find((r) => r.kind === 'tiles' && r.section === 1)!.top);
  });

  // The space between two folders belongs to no row; the row above it is as far as the pin
  // can say, so it says the whole of it rather than a share past its end.
  it('takes the gap under a folder as the end of its last row', () => {
    const medium = buildRows(folders, 3, TILE_WIDTH.medium);
    const last = medium.filter((r) => r.kind === 'tiles' && r.section === 0).at(-1)!;
    expect(pinAt(medium, last.top + last.height + 10)).toEqual({ offset: last.first, header: false, into: 1 });
  });

  it('has nowhere to come back to once its photo is gone', () => {
    expect(pinTop(buildRows(folders, 3, TILE_WIDTH.medium), { offset: 99, header: false, into: 0 })).toBeNull();
  });
});

describe('tiles that fill the row', () => {
  it('widens the tiles to take up what the columns leave over', () => {
    // Four medium tiles and three gaps are 664; the 136 left over is 34 a tile.
    expect(columnsFor(800, TILE_WIDTH.medium)).toBe(4);
    expect(tileFor(800, TILE_WIDTH.medium)).toBe(194);
  });

  it('keeps the chosen width in a row it already fills', () => {
    expect(tileFor(4 * 160 + 3 * GAP, TILE_WIDTH.medium)).toBe(160);
  });

  it('is a whole number of pixels, leaving less than a pixel a column', () => {
    for (const width of [801, 802, 803]) {
      const tile = tileFor(width, TILE_WIDTH.medium);
      expect(Number.isInteger(tile)).toBe(true);
      const left = width - (4 * tile + 3 * GAP);
      expect(left).toBeGreaterThanOrEqual(0);
      expect(left).toBeLessThan(4);
    }
  });

  it('never draws a tile narrower than the size chosen', () => {
    expect(tileFor(100, TILE_WIDTH.medium)).toBe(160);
    expect(tileFor(0, TILE_WIDTH.large)).toBe(224);
  });

  // The row is laid out for `columnsFor` columns, so the widened tiles have to be that many
  // and fit: a tile widened past its share would push the last one of each row off the edge.
  it('fits the columns the row was laid out for, at every width', () => {
    for (const nominal of Object.values(TILE_WIDTH)) {
      for (let width = nominal; width <= 3000; width++) {
        const columns = columnsFor(width, nominal);
        const tile = tileFor(width, nominal);
        expect(tile, `${nominal} at ${width}`).toBeGreaterThanOrEqual(nominal);
        expect(columns * tile + (columns - 1) * GAP, `${nominal} at ${width}`).toBeLessThanOrEqual(width);
      }
    }
  });

  // A grid thumbnail is 256px on its long edge: a tile drawn much wider than that shows it
  // enlarged. Three large tiles in 900px would be 294 each.
  it('stops an eighth past the thumbnail it draws', () => {
    expect(TILE_MAX).toBe(288);
    expect(columnsFor(900, TILE_WIDTH.large)).toBe(3);
    expect(tileFor(900, TILE_WIDTH.large)).toBe(288);
    expect(tileFor(860, TILE_WIDTH.large)).toBe(281);
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

describe('period sections', () => {
  const month = (m: number, offset: number, count: number) => ({ folderId: null, period: { year: 2026, month: m, day: null }, offset, count });

  it('get a header each, as a folder does', () => {
    const rows = buildRows([month(10, 0, 3), month(9, 3, 2)], 2, TILE_WIDTH.medium);
    expect(rows.map((r) => r.kind)).toEqual(['header', 'tiles', 'tiles', 'header', 'tiles']);
    expect(rows[3].first).toBe(3);
  });

  it('and a run with neither a folder nor a period gets none', () => {
    const rows = buildRows([{ folderId: null, period: null, offset: 0, count: 3 }], 2, TILE_WIDTH.medium);
    expect(rows.map((r) => r.kind)).toEqual(['tiles', 'tiles']);
    expect(hasHeader({ folderId: null, offset: 0, count: 3 })).toBe(false);
    expect(hasHeader({ folderId: 7, offset: 0, count: 3 })).toBe(true);
  });
});

describe('the height of a layout without building it', () => {
  // `showsTimeline` asks how tall the grid would be at another width, on every resize, in a
  // library of any size: it must be the height `buildRows` arrives at, row by row.
  it('is the height the rows come to, for every shape of section', () => {
    const shapes = [
      [],
      sections,
      headerless(sections),
      [{ folderId: 1, offset: 0, count: 1 }],
      [{ folderId: 1, offset: 0, count: 0 }, { folderId: 2, offset: 0, count: 7 }],
      [sections[0], { folderId: null, offset: 5, count: 3 }],
      // A header after a run that has none still gets the gap above it.
      [{ folderId: null, offset: 0, count: 3 }, { folderId: 2, offset: 3, count: 3 }],
      [{ folderId: null, period: { year: 2026, month: 7, day: null }, offset: 0, count: 23 }, { folderId: null, period: { year: 2026, month: 3, day: null }, offset: 23, count: 11 }],
    ];
    for (const shape of shapes) {
      for (const columns of [1, 2, 3, 7]) {
        for (const tile of [120, 181, 224]) {
          expect(layoutHeight(shape, columns, tile), `${JSON.stringify(shape)} at ${columns} x ${tile}`).toBe(
            totalHeight(buildRows(shape, columns, tile)),
          );
        }
      }
    }
  });
});

describe('whether the year strip is shown', () => {
  // Two years, nine photos each, so two rows of five each. Beside the strip the grid is 956
  // wide and its tiles 181; with no strip 1000 and 190: four rows of those are 36px taller.
  const two = [
    { folderId: 1, offset: 0, count: 9 },
    { folderId: 2, offset: 9, count: 9 },
  ];
  const outer = 1000;
  const beside = layoutHeight(two, 5, 181);
  const alone = layoutHeight(two, 5, 190);

  it('lays the example out as the cases below assume', () => {
    const row = rowWidth(outer - TIMELINE_WIDTH);
    expect([columnsFor(row, TILE_WIDTH.medium), tileFor(row, TILE_WIDTH.medium)]).toEqual([5, 181]);
    expect([columnsFor(rowWidth(outer), TILE_WIDTH.medium), tileFor(rowWidth(outer), TILE_WIDTH.medium)]).toEqual([5, 190]);
    expect(alone).toBeGreaterThan(beside);
  });

  it('is shown when the grid beside it has something to scroll', () => {
    expect(showsTimeline(two, 2, outer, 0, beside - 1, TILE_WIDTH.medium)).toBe(true);
  });

  // The strip takes width from the tiles, and tiles that fill the row are shorter for it:
  // in this band the grid fits beside the strip and overflows without it. Asked of the grid
  // as it stood, the answer changed the grid, which changed the answer - the strip came
  // and went on every frame. It is asked of the grid as it would be beside the strip, which
  // showing the strip does not change.
  it('is not shown while the grid would fit beside it, though it overflows without', () => {
    expect(beside).toBeLessThan(alone);
    for (const viewport of [beside, beside + 1, alone - 1]) {
      expect(showsTimeline(two, 2, outer, 0, viewport, TILE_WIDTH.medium), `${viewport}`).toBe(false);
    }
  });

  it('needs more than one year to choose between', () => {
    expect(showsTimeline(two, 1, outer, 0, 10, TILE_WIDTH.medium)).toBe(false);
    expect(showsTimeline(two, 0, outer, 0, 10, TILE_WIDTH.medium)).toBe(false);
  });

  // A scrollbar that takes room narrows the grid as the strip does.
  it('counts the room a scrollbar takes', () => {
    const row = rowWidth(outer - TIMELINE_WIDTH - 15);
    const narrower = layoutHeight(two, columnsFor(row, TILE_WIDTH.medium), tileFor(row, TILE_WIDTH.medium));
    expect(narrower).toBeLessThan(beside);
    expect(showsTimeline(two, 2, outer, 15, narrower, TILE_WIDTH.medium)).toBe(false);
    expect(showsTimeline(two, 2, outer, 15, narrower - 1, TILE_WIDTH.medium)).toBe(true);
  });
});

describe('where the grid is drawn after its rows change height', () => {
  const run = [{ folderId: null, offset: 0, count: 300 }];
  const before = buildRows(run, 5, 181);
  const after = buildRows(run, 5, 169);

  it('is where it was scrolled to while the tiles have not changed', () => {
    expect(placeIn(before, 800, 12345, pinAt(before, 3000), false)).toBe(12345);
  });

  // The rows are new on the very render that follows a width change, and the viewport's own
  // position is not corrected until an effect after it: drawn at the old number, that
  // render showed other rows - blank ones, deep in a library - for a frame, on every frame.
  it("is the pin's place in the new rows from the first render with them", () => {
    const pin = pinAt(before, 40 * tileRow(181) + 50)!;
    expect(placeIn(after, 800, 40 * tileRow(181) + 50, pin, true)).toBe(pinTop(after, pin));
    expect(placeIn(after, 800, 40 * tileRow(181) + 50, pin, true)).not.toBe(40 * tileRow(181) + 50);
  });

  it('stops where the browser will stop, at the end of the grid', () => {
    const end = totalHeight(before) - 10;
    const pin = pinAt(before, end)!;
    expect(pinTop(after, pin)!).toBeGreaterThan(totalHeight(after) - 800);
    expect(placeIn(after, 800, end, pin, true)).toBe(totalHeight(after) - 800);
    // A grid shorter than its viewport does not scroll at all.
    const short = buildRows([{ folderId: null, offset: 0, count: 5 }], 5, 181);
    expect(placeIn(short, 800, 0, { offset: 0, header: false, into: 0.5 }, true)).toBe(0);
  });

  it('stays where it was with no pin, or one whose photo has gone', () => {
    expect(placeIn(after, 800, 500, null, true)).toBe(500);
    expect(placeIn(after, 800, 500, { offset: 9999, header: false, into: 0 }, true)).toBe(500);
  });
});

describe('the header pinned to the top of the grid', () => {
  // Two folders at two columns of medium tiles: header 0, rows at 32, 200, 368; then the
  // section gap, header at 560, rows at 592 and 760.
  const rows = buildRows(sections, 2, TILE_WIDTH.medium);
  const headers = headerRows(rows);
  const second = rows[headers[1]];

  it('lists the header rows, in order', () => {
    expect(headers.map((i) => [rows[i].kind, rows[i].section, rows[i].top])).toEqual([
      ['header', 0, 0],
      ['header', 1, 560],
    ]);
    expect(headerRows(buildRows(headerless(sections), 2, TILE_WIDTH.medium))).toEqual([]);
  });

  // The real header is there, exactly where the pinned one would be drawn.
  it('pins nothing while the section header itself is at the top', () => {
    expect(pinnedHeader(rows, headers, 0)).toBeNull();
    expect(pinnedHeader(rows, headers, second.top)).toBeNull();
  });

  it('pins the header of the section the top of the grid is inside', () => {
    expect(pinnedHeader(rows, headers, 1)).toEqual({ section: 0, y: 0 });
    expect(pinnedHeader(rows, headers, 300)).toEqual({ section: 0, y: 0 });
    expect(pinnedHeader(rows, headers, second.top + 1)).toEqual({ section: 1, y: 0 });
    expect(pinnedHeader(rows, headers, 900)).toEqual({ section: 1, y: 0 });
  });

  // In the space between two folders it is still the folder above that the eye is leaving.
  it('keeps the folder above pinned through the gap under its last row', () => {
    const lastRowEnd = second.top - SECTION_GAP;
    expect(pinnedHeader(rows, headers, lastRowEnd + 1)?.section).toBe(0);
  });

  // The next header arriving pushes the pinned one out, so the two never overlap: the
  // pinned header's bottom edge rides on the arriving header's top.
  it('is pushed up by the next header as it arrives', () => {
    expect(pinnedHeader(rows, headers, second.top - HEADER)).toEqual({ section: 0, y: 0 });
    expect(pinnedHeader(rows, headers, second.top - HEADER + 10)).toEqual({ section: 0, y: -10 });
    expect(pinnedHeader(rows, headers, second.top - 1)).toEqual({ section: 0, y: -(HEADER - 1) });
  });

  it('pins nothing in a grid with no headers, or with no rows', () => {
    const flat = buildRows(headerless(sections), 2, TILE_WIDTH.medium);
    expect(pinnedHeader(flat, headerRows(flat), 300)).toBeNull();
    expect(pinnedHeader([], [], 0)).toBeNull();
  });

  // A run with no header after one with: nothing above it is its header.
  it('pins nothing over a run that has no header of its own', () => {
    const mixed = buildRows([sections[0], { folderId: null, offset: 5, count: 3 }], 2, TILE_WIDTH.medium);
    const run = mixed.find((r) => r.kind === 'tiles' && r.section === 1)!;
    expect(pinnedHeader(mixed, headerRows(mixed), run.top + 10)).toBeNull();
    expect(pinnedHeader(mixed, headerRows(mixed), 100)).toEqual({ section: 0, y: 0 });
  });
});

describe('scrolling a row into view', () => {
  const rows = buildRows(sections, 2, TILE_WIDTH.medium);
  const tiles = rows.filter((r) => r.kind === 'tiles');
  const viewport = 400;

  it('leaves a row alone that is already wholly in view', () => {
    expect(scrollIntoGrid(tiles[1], 150, viewport, HEADER)).toBeNull();
  });

  // The pinned header lies over the top of the grid: a row brought flush to the top would
  // be under it, selected and half out of sight.
  it('brings a row above the view down to just under the pinned header', () => {
    expect(scrollIntoGrid(tiles[1], 300, viewport, HEADER)).toBe(tiles[1].top - HEADER);
  });

  it('takes a row behind the pinned header for one out of view', () => {
    // 10px of the row is above the fold line under the header, all of it below the top edge.
    expect(scrollIntoGrid(tiles[1], tiles[1].top - HEADER + 10, viewport, HEADER)).toBe(tiles[1].top - HEADER);
    // Flush under the header is in view.
    expect(scrollIntoGrid(tiles[1], tiles[1].top - HEADER, viewport, HEADER)).toBeNull();
  });

  // A section's first row has its own header directly above it, `HEADER` tall: the same
  // sum lands on that header's top, so the real header shows where the pinned one would.
  it("brings a section's first row into view with its header", () => {
    const header = rows.find((r) => r.kind === 'header' && r.section === 1)!;
    const first = rows.find((r) => r.kind === 'tiles' && r.section === 1)!;
    expect(scrollIntoGrid(first, first.top + 50, viewport, HEADER)).toBe(header.top);
  });

  it('brings a row below the view up until its bottom edge shows', () => {
    expect(scrollIntoGrid(tiles[2], 0, 300, HEADER)).toBe(tiles[2].top + tiles[2].height - 300);
  });

  it('goes flush to the top where nothing is pinned', () => {
    expect(scrollIntoGrid(tiles[1], 300, viewport, 0)).toBe(tiles[1].top);
    expect(scrollIntoGrid(tiles[1], tiles[1].top, viewport, 0)).toBeNull();
  });

  it('never asks for a place above the start of the grid', () => {
    expect(scrollIntoGrid({ ...tiles[0], top: 10 }, 200, viewport, HEADER)).toBe(0);
  });
});

describe('scrolling a row to the start of the view', () => {
  const rows = buildRows(sections, 2, TILE_WIDTH.medium);

  it("puts a section's first row under its own header", () => {
    const header = rows.find((r) => r.kind === 'header' && r.section === 1)!;
    const first = rows.find((r) => r.kind === 'tiles' && r.section === 1)!;
    expect(scrollToStart(first, HEADER)).toBe(header.top);
    expect(scrollToStart(rows[1], HEADER)).toBe(0);
  });

  // A folder jump under a date grouping lands on the folder's first photo, which is in the
  // middle of a month: flush to the top, the pinned header covered the top of that row.
  it('puts a row from the middle of a section under the pinned header, not behind it', () => {
    const middle = rows.filter((r) => r.kind === 'tiles' && r.section === 0)[1];
    expect(scrollToStart(middle, HEADER)).toBe(middle.top - HEADER);
  });

  it('puts a row flush to the top where there are no headers', () => {
    const flat = buildRows(headerless(sections), 2, TILE_WIDTH.medium);
    expect(scrollToStart(flat[2], 0)).toBe(flat[2].top);
    expect(scrollToStart(flat[0], 0)).toBe(0);
  });
});

describe('a page key in the grid', () => {
  // Five rows of two in the first folder, three in the second: tile rows are 168px tall.
  const runs = [
    { folderId: 1, offset: 0, count: 10 },
    { folderId: 2, offset: 10, count: 5 },
  ];
  const rows = buildRows(runs, 2, TILE_WIDTH.medium);

  it('moves down by the rows a viewport holds, keeping the column', () => {
    // 400px is two rows and a bit: from the first row to the third, not the fourth.
    expect(pageMove(rows, 1, 1, 400)).toBe(5);
    expect(pageMove(rows, 0, 1, 400)).toBe(4);
  });

  it('moves up by as many', () => {
    expect(pageMove(rows, 5, -1, 400)).toBe(1);
    expect(pageMove(rows, 9, -1, 400)).toBe(5);
  });

  it('crosses a header, whose height is part of the page', () => {
    // From the folder's last row (top 704) a page of 400 reaches the next folder's first
    // row (top 928) and its second (1096), and stops short of the third (1264).
    expect(pageMove(rows, 9, 1, 400)).toBe(13);
  });

  it('lands on the last photo of a row too short to have the column', () => {
    // The second folder ends on a row of one.
    expect(pageMove(rows, 11, 1, 400)).toBe(14);
  });

  it('stops at the first and the last row', () => {
    expect(pageMove(rows, 3, -1, 400)).toBe(1);
    expect(pageMove(rows, 13, 1, 400)).toBe(14);
    expect(pageMove(rows, 14, 1, 400)).toBe(14);
    expect(pageMove(rows, 0, -1, 400)).toBe(0);
  });

  it('moves a row in a window shorter than one', () => {
    // A key that moves nothing reads as broken, whatever the window's height.
    expect(pageMove(rows, 0, 1, 100)).toBe(2);
    expect(pageMove(rows, 4, -1, 100)).toBe(2);
    expect(pageMove(rows, 0, 1, 0)).toBe(2);
  });

  it('leaves alone an offset the rows do not hold', () => {
    expect(pageMove(rows, 99, 1, 400)).toBe(99);
    expect(pageMove([], 0, 1, 400)).toBe(0);
  });
});
