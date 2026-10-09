//! Grid geometry: square tiles in rows of one height, with a header row for every section
//! that names a folder or a period. `ui/src/lib/layout.ts` in Rust, constant for constant,
//! until the switch-over deletes that file.
//!
//! Everything is pure, and in `f64`: a large library is taller than an `f32` can count
//! pixels in (it is exact only to 16,777,216), and every `top` here is such a count.

use super::motion::{Direction, Motion};
use photon_core::{grid::Section, library::GridTile};

pub const GAP: f64 = 8.0;
pub const HEADER: f64 = 32.0;
/// Extra space above every section's header but the first, on top of the gutter under the
/// last row before it, so one folder reads as ending before the next begins.
pub const SECTION_GAP: f64 = 24.0;
/// The widest a tile is drawn: an eighth past `ThumbSize::Grid`'s 256px edge, which is how
/// much of an enlargement goes unseen.
pub const TILE_MAX: f64 = 288.0;
/// How long a tile must be wanted before its thumbnail is asked for, where asking is
/// deferred at all (`defers_thumbs`).
pub const TILE_SETTLE_MS: f64 = 100.0;
/// How far past the viewport thumbnails are wanted while the grid is still, in viewports,
/// either side: enough that a wheel notch lands on rows that have their pictures.
pub const RENDER_OVERSCAN: f64 = 0.5;
/// While scrolling, how far ahead of the direction of travel thumbnails are wanted, as
/// time: a tile wanted this long before it comes into view has been read and decoded
/// before anyone sees it.
pub const LEAD_MS: f64 = 300.0;
/// The most the lead grows to, in viewports.
pub const LEAD_OVERSCAN_MAX: f64 = 1.5;
/// What stays wanted behind a scroll, in viewports: a small reversal lands on rows that
/// still have their pictures.
pub const TRAIL_OVERSCAN: f64 = 0.25;

/// How wide a tile is at each size, before it is widened to fill its row (`tile_for`).
///
/// Every step is at or below 256, `ThumbSize::Grid`'s maximum edge: a step above it needs
/// its own `ThumbSize`, not a larger `Grid`.
pub fn tile_width(size: GridTile) -> f64 {
    match size {
        GridTile::Small => 120.0,
        GridTile::Medium => 160.0,
        GridTile::Large => 224.0,
    }
}

/// A tile row's full height: the tile plus the gutter under it.
pub fn tile_row(tile: f64) -> f64 {
    tile + GAP
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Header,
    Tiles,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pub kind: RowKind,
    /// Index into the sections.
    pub section: usize,
    /// Grid offset of the first photo (the section's offset, for a header).
    pub first: usize,
    /// Photos in this row (0 for a header).
    pub count: usize,
    pub top: f64,
    pub height: f64,
}

pub fn columns_for(width: f64, tile: f64) -> usize {
    (((width + GAP) / tile_row(tile)).floor() as usize).max(1)
}

/// How wide a tile is drawn in a row `width` wide: the size chosen, widened so the columns
/// that fit fill the row. A whole number, so every row's `top` is one too; never narrower
/// than the size chosen, which a row too narrow for one tile overflows.
pub fn tile_for(width: f64, nominal: f64) -> f64 {
    let columns = columns_for(width, nominal) as f64;
    let share = ((width - (columns - 1.0) * GAP) / columns).floor();
    nominal.max(share.min(TILE_MAX))
}

/// A section has a header when it names a folder or a period; a flat view's run names
/// neither.
pub fn has_header(section: &Section) -> bool {
    section.folder_id.is_some() || section.period.is_some()
}

pub fn build_rows(sections: &[Section], columns: usize, tile: f64) -> Vec<Row> {
    let columns = columns.max(1);
    let mut rows = Vec::new();
    let mut top = 0.0;
    for (index, section) in sections.iter().enumerate() {
        if has_header(section) {
            if !rows.is_empty() {
                top += SECTION_GAP;
            }
            rows.push(Row {
                kind: RowKind::Header,
                section: index,
                first: section.offset,
                count: 0,
                top,
                height: HEADER,
            });
            top += HEADER;
        }
        let end = section.offset + section.count;
        let mut first = section.offset;
        while first < end {
            rows.push(Row {
                kind: RowKind::Tiles,
                section: index,
                first,
                count: columns.min(end - first),
                top,
                height: tile_row(tile),
            });
            top += tile_row(tile);
            first += columns;
        }
    }
    rows
}

/// The width of a row of tiles in a viewport `viewport` wide: what is left between the
/// gutter down either side.
pub fn row_width(viewport: f64) -> f64 {
    (viewport - 2.0 * GAP).max(0.0)
}

pub fn total_height(rows: &[Row]) -> f64 {
    rows.last().map_or(0.0, |last| last.top + last.height)
}

/// Index of the last item whose `key` is at or below `value`, or 0 if there is none.
pub fn last_index_at_or_before<T>(items: &[T], value: f64, key: impl Fn(&T) -> f64) -> usize {
    items
        .partition_point(|item| key(item) <= value)
        .saturating_sub(1)
}

/// Index of the last row whose top is at or below `y`: the row holding `y`.
pub fn row_index_at(rows: &[Row], y: f64) -> usize {
    last_index_at_or_before(rows, y, |row| row.top)
}

/// Rows intersecting `[top - overscan, top + viewport + overscan]`, as `start..end`.
pub fn visible_range(rows: &[Row], top: f64, viewport: f64, overscan: f64) -> (usize, usize) {
    if rows.is_empty() {
        return (0, 0);
    }
    let start = row_index_at(rows, top - overscan);
    let end = row_index_at(rows, top + viewport + overscan) + 1;
    (start, end.min(rows.len()))
}

/// How far past the viewport thumbnails are wanted, as `(above, below)`.
///
/// A jump wants only what is on screen: it shares no rows with the frame before it, and
/// during a drag the next frame replaces all of them. A continuous scroll needs a lead:
/// without one every row reaches the screen before its tiles have asked for anything.
pub fn wanted_overscan(motion: Motion, viewport: f64) -> (f64, f64) {
    match motion {
        Motion::Still => (viewport * RENDER_OVERSCAN, viewport * RENDER_OVERSCAN),
        Motion::Jump { .. } => (0.0, 0.0),
        Motion::Scroll {
            direction, peak, ..
        } => {
            let lead = (peak * LEAD_MS)
                .max(viewport * RENDER_OVERSCAN)
                .min(viewport * LEAD_OVERSCAN_MAX);
            let trail = viewport * TRAIL_OVERSCAN;
            match direction {
                Direction::Down => (trail, lead),
                Direction::Up => (lead, trail),
            }
        }
    }
}

/// The rows whose thumbnails are wanted, as `start..end`; see `wanted_overscan`.
pub fn wanted_range(rows: &[Row], top: f64, viewport: f64, motion: Motion) -> (usize, usize) {
    if rows.is_empty() {
        return (0, 0);
    }
    let (above, below) = wanted_overscan(motion, viewport);
    let start = row_index_at(rows, top - above);
    let end = row_index_at(rows, top + viewport + below) + 1;
    (start, end.min(rows.len()))
}

/// Whether the tiles in the wanted range wait before asking for their thumbnails.
///
/// Only when they will most likely be gone first: a tile never seen settled costs a render
/// for nothing - on a fresh import, a blocking one each. That is a jump in a stream (a
/// scrollbar drag), where the next frame replaces every tile, and a scroll so fast that a
/// tile crosses the whole wanted window in less than the settle. A jump on its own (End)
/// lands where the user stops, so it asks at once.
pub fn defers_thumbs(motion: Motion, viewport: f64) -> bool {
    match motion {
        Motion::Still => false,
        Motion::Jump { stream } => stream,
        Motion::Scroll { speed, .. } => {
            let (above, below) = wanted_overscan(motion, viewport);
            speed * TILE_SETTLE_MS > viewport + above + below
        }
    }
}

/// Grid offsets covered by the tile rows in `rows`, as `start..end`, or `None` if none.
pub fn item_span(rows: &[Row]) -> Option<(usize, usize)> {
    let mut span: Option<(usize, usize)> = None;
    for row in rows.iter().filter(|row| row.kind == RowKind::Tiles) {
        let (start, end) = span.unwrap_or((usize::MAX, 0));
        span = Some((start.min(row.first), end.max(row.first + row.count)));
    }
    span
}

/// Index of the tile row holding grid offset `offset`.
pub fn row_of_item(rows: &[Row], offset: usize) -> Option<usize> {
    let found = rows
        .partition_point(|row| row.first <= offset)
        .checked_sub(1)?;
    let row = &rows[found];
    (row.kind == RowKind::Tiles && offset < row.first + row.count).then_some(found)
}

/// The indexes of the header rows in `rows`, in order: what `pinned_header` searches, built
/// once per layout.
pub fn header_rows(rows: &[Row]) -> Vec<usize> {
    (0..rows.len())
        .filter(|&i| rows[i].kind == RowKind::Header)
        .collect()
}

/// The header drawn over the top of the grid: its section, and how far up it has been
/// pushed (`y`, zero or negative).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PinnedHeader {
    pub section: usize,
    pub y: f64,
}

/// The header to pin to the top of the grid at `top`, or `None` for none.
///
/// It is the header of the section the top edge is inside - the space under a section's
/// last row included, where it is still that section the eye is leaving - and none while
/// that header is itself at the top, or over a run that has no header. The next header
/// pushes it out as it arrives, so the two never lie over each other.
pub fn pinned_header(rows: &[Row], headers: &[usize], top: f64) -> Option<PinnedHeader> {
    if headers.is_empty() || rows.is_empty() {
        return None;
    }
    let at = last_index_at_or_before(headers, top, |&i| rows[i].top);
    let header = &rows[headers[at]];
    if top <= header.top {
        return None;
    }
    if rows[row_index_at(rows, top)].section != header.section {
        return None;
    }
    let y = headers
        .get(at + 1)
        .map_or(0.0, |&next| (rows[next].top - top - HEADER).min(0.0));
    Some(PinnedHeader {
        section: header.section,
        y,
    })
}

/// A place in the grid that survives the rows changing height: the row at the top of the
/// viewport, by its first photo, and how much of it has been scrolled past.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pin {
    /// The grid offset of the row's first photo (for a header, of the section it heads).
    pub offset: usize,
    /// Whether the row is a header: a header and the row of photos under it share an offset.
    pub header: bool,
    /// The share of the row above the top of the viewport, 0 to 1.
    pub into: f64,
}

/// The row at the top of the viewport as a `Pin`, or `None` when the grid has no rows.
///
/// Tiles fill the row, so every row's `top` moves with every pixel of a resize, and a
/// position kept as a number names another photo afterwards. The share is kept because a
/// resize spends the pin on every frame: coming back to the row's top would jump by up to a
/// row on the first one. Past the end of the row - the space between two folders - is the
/// whole of it.
pub fn pin_at(rows: &[Row], top: f64) -> Option<Pin> {
    let row = rows.get(row_index_at(rows, top))?;
    Some(Pin {
        offset: row.first,
        header: row.kind == RowKind::Header,
        into: ((top - row.top) / row.height).clamp(0.0, 1.0),
    })
}

/// Where `pin` is in `rows`, as a position, or `None` when its photo is not there.
pub fn pin_top(rows: &[Row], pin: Pin) -> Option<f64> {
    let i = row_of_item(rows, pin.offset)?;
    let above = i.checked_sub(1).map(|above| &rows[above]);
    let row = match above {
        Some(above) if pin.header && above.kind == RowKind::Header && above.first == pin.offset => {
            above
        }
        _ => &rows[i],
    };
    Some(row.top + pin.into * row.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEDIUM: f64 = 160.0;
    const SMALL: f64 = 120.0;
    const LARGE: f64 = 224.0;

    fn folder(folder_id: i64, offset: usize, count: usize) -> Section {
        Section {
            folder_id: Some(folder_id),
            offset,
            count,
            taken_at_min: 0,
            period: None,
        }
    }

    /// A run drawn under no header, as a flat view's is.
    fn flat(offset: usize, count: usize) -> Section {
        Section {
            folder_id: None,
            ..folder(0, offset, count)
        }
    }

    fn two_folders() -> Vec<Section> {
        vec![folder(1, 0, 5), folder(2, 5, 3)]
    }

    fn shape(rows: &[Row]) -> Vec<(RowKind, usize, usize, f64)> {
        rows.iter()
            .map(|r| (r.kind, r.first, r.count, r.top))
            .collect()
    }

    #[test]
    fn sizes_are_the_svelte_grids() {
        assert_eq!(tile_width(GridTile::Small), SMALL);
        assert_eq!(tile_width(GridTile::Medium), MEDIUM);
        assert_eq!(tile_width(GridTile::Large), LARGE);
    }

    #[test]
    fn columns_fit_the_width() {
        assert_eq!(columns_for(800.0, MEDIUM), 4);
        assert_eq!(columns_for(100.0, MEDIUM), 1);
        assert_eq!(columns_for(0.0, MEDIUM), 1);
        assert_eq!(columns_for(-50.0, MEDIUM), 1);
    }

    #[test]
    fn a_section_is_a_header_row_and_its_tile_rows() {
        use RowKind::{Header, Tiles};
        let rows = build_rows(&two_folders(), 2, MEDIUM);
        assert_eq!(
            shape(&rows),
            [
                (Header, 0, 0, 0.0),
                (Tiles, 0, 2, 32.0),
                (Tiles, 2, 2, 200.0),
                (Tiles, 4, 1, 368.0),
                // SECTION_GAP (24) above every header but the first: folder 1 ends at 536.
                (Header, 5, 0, 560.0),
                (Tiles, 5, 2, 592.0),
                (Tiles, 7, 1, 760.0),
            ]
        );
        assert_eq!(total_height(&rows), 928.0);
        assert_eq!(total_height(&[]), 0.0);
    }

    #[test]
    fn a_run_that_names_no_folder_has_no_header() {
        use RowKind::Tiles;
        let rows = build_rows(&[flat(0, 5)], 2, MEDIUM);
        assert_eq!(
            shape(&rows),
            [
                (Tiles, 0, 2, 0.0),
                (Tiles, 2, 2, 168.0),
                (Tiles, 4, 1, 336.0)
            ]
        );
        assert_eq!(total_height(&rows), 504.0);
    }

    #[test]
    fn a_period_gets_a_header_as_a_folder_does() {
        let period = Section {
            period: Some(photon_core::grid::Period {
                year: 2024,
                month: Some(6),
                day: None,
            }),
            ..flat(0, 3)
        };
        assert!(has_header(&period));
        assert!(!has_header(&flat(0, 3)));
        assert_eq!(build_rows(&[period], 2, MEDIUM)[0].kind, RowKind::Header);
    }

    #[test]
    fn rows_are_hit_by_y() {
        let rows = build_rows(&two_folders(), 2, MEDIUM);
        assert_eq!(row_index_at(&rows, -5.0), 0);
        assert_eq!(row_index_at(&rows, 31.0), 0);
        assert_eq!(row_index_at(&rows, 32.0), 1);
        assert_eq!(row_index_at(&rows, 500.0), 3);
        assert_eq!(row_index_at(&rows, 10_000.0), 6);
    }

    #[test]
    fn visible_ranges_and_their_photos() {
        let rows = build_rows(&two_folders(), 2, MEDIUM);
        assert_eq!(visible_range(&rows, 200.0, 100.0, 0.0), (2, 3));
        assert_eq!(visible_range(&rows, 0.0, 10_000.0, 0.0), (0, 7));
        assert_eq!(visible_range(&[], 0.0, 100.0, 0.0), (0, 0));
        assert_eq!(item_span(&rows[0..3]), Some((0, 4)));
        assert_eq!(item_span(&rows[4..5]), None);
    }

    #[test]
    fn the_row_holding_a_photo() {
        let rows = build_rows(&two_folders(), 2, MEDIUM);
        assert_eq!(row_of_item(&rows, 0), Some(1));
        assert_eq!(row_of_item(&rows, 4), Some(3));
        assert_eq!(row_of_item(&rows, 5), Some(5));
        assert_eq!(row_of_item(&rows, 7), Some(6));
        assert_eq!(row_of_item(&rows, 8), None);
        assert_eq!(row_of_item(&[], 0), None);
    }

    mod wanted {
        use super::*;

        // One flat run of 1000 rows, 168px each: a 1680px viewport is ten rows.
        fn rows() -> Vec<Row> {
            build_rows(&[flat(0, 1000)], 1, MEDIUM)
        }
        const ROW: f64 = 168.0;
        const VIEWPORT: f64 = 10.0 * ROW;
        const TOP: f64 = 100.0 * ROW;

        fn scroll(direction: Direction, speed: f64, peak: f64) -> Motion {
            Motion::Scroll {
                direction,
                speed,
                peak,
            }
        }

        #[test]
        fn half_a_viewport_past_the_edges_while_still() {
            assert_eq!(
                wanted_range(&rows(), TOP, VIEWPORT, Motion::Still),
                (95, 116)
            );
        }

        #[test]
        fn only_what_is_on_screen_for_a_jump() {
            for stream in [true, false] {
                let jump = Motion::Jump { stream };
                assert_eq!(wanted_range(&rows(), TOP, VIEWPORT, jump), (100, 111));
            }
        }

        #[test]
        fn a_lead_ahead_of_a_fast_scroll_and_a_little_behind_it() {
            // 4px/ms leads by LEAD_MS of travel (1200px) and trails by a quarter viewport.
            let down = scroll(Direction::Down, 4.0, 4.0);
            assert_eq!(
                wanted_overscan(down, VIEWPORT),
                (VIEWPORT * TRAIL_OVERSCAN, 4.0 * LEAD_MS)
            );
            assert_eq!(wanted_range(&rows(), TOP, VIEWPORT, down), (97, 118));
            // Upward, the lead is above.
            let up = scroll(Direction::Up, 4.0, 4.0);
            assert_eq!(wanted_range(&rows(), TOP, VIEWPORT, up), (92, 113));
        }

        #[test]
        fn the_lead_is_at_least_the_still_overscan_and_at_most_its_maximum() {
            let below = |speed| wanted_overscan(scroll(Direction::Down, speed, speed), VIEWPORT).1;
            assert_eq!(below(0.0), VIEWPORT * RENDER_OVERSCAN);
            assert_eq!(below(0.1), VIEWPORT * RENDER_OVERSCAN);
            assert_eq!(below(100.0), VIEWPORT * LEAD_OVERSCAN_MAX);
        }

        #[test]
        fn the_lead_is_sized_by_the_peak_so_a_flick_slowing_down_keeps_it() {
            assert_eq!(
                wanted_range(&rows(), TOP, VIEWPORT, scroll(Direction::Down, 0.5, 4.0)),
                wanted_range(&rows(), TOP, VIEWPORT, scroll(Direction::Down, 4.0, 4.0)),
            );
        }

        #[test]
        fn thumbnails_wait_only_where_a_tile_will_be_gone_first() {
            assert!(!defers_thumbs(Motion::Still, VIEWPORT));
            // End lands where the user stops.
            assert!(!defers_thumbs(Motion::Jump { stream: false }, VIEWPORT));
            // A scrollbar drag replaces every tile on the next frame.
            assert!(defers_thumbs(Motion::Jump { stream: true }, VIEWPORT));
            // A 4px/ms flick passes a tile through the window in over half a second.
            assert!(!defers_thumbs(scroll(Direction::Down, 4.0, 4.0), VIEWPORT));
            // The window is 2.75 viewports at full lead: crossed within the settle above
            // 46.2px/ms.
            let window = VIEWPORT * (1.0 + LEAD_OVERSCAN_MAX + TRAIL_OVERSCAN);
            let at = |speed| defers_thumbs(scroll(Direction::Down, speed, speed), VIEWPORT);
            assert!(!at(window / TILE_SETTLE_MS - 0.1));
            assert!(at(window / TILE_SETTLE_MS + 0.1));
        }
    }

    mod filling_the_row {
        use super::*;

        #[test]
        fn tiles_widen_to_take_up_what_the_columns_leave_over() {
            // Four medium tiles and three gaps are 664; the 136 left over is 34 a tile.
            assert_eq!(columns_for(800.0, MEDIUM), 4);
            assert_eq!(tile_for(800.0, MEDIUM), 194.0);
        }

        #[test]
        fn a_row_already_filled_keeps_the_chosen_width() {
            assert_eq!(tile_for(4.0 * 160.0 + 3.0 * GAP, MEDIUM), 160.0);
        }

        #[test]
        fn a_tile_is_never_narrower_than_the_size_chosen() {
            assert_eq!(tile_for(100.0, MEDIUM), 160.0);
            assert_eq!(tile_for(0.0, LARGE), 224.0);
        }

        // The row is laid out for `columns_for` columns, so the widened tiles have to be
        // that many and fit: a tile widened past its share would push the last one of each
        // row off the edge.
        #[test]
        fn the_columns_fit_at_every_width_in_whole_pixels() {
            for nominal in [SMALL, MEDIUM, LARGE] {
                for width in nominal as u32..=3000 {
                    let width = f64::from(width);
                    let columns = columns_for(width, nominal) as f64;
                    let tile = tile_for(width, nominal);
                    assert_eq!(tile.fract(), 0.0, "{nominal} at {width}");
                    assert!(tile >= nominal, "{nominal} at {width}");
                    assert!(
                        columns * tile + (columns - 1.0) * GAP <= width,
                        "{nominal} at {width}"
                    );
                }
            }
        }

        // A grid thumbnail is 256px on its long edge: a tile drawn much wider shows it
        // enlarged. Three large tiles in 900px would be 294 each.
        #[test]
        fn a_tile_stops_an_eighth_past_the_thumbnail_it_draws() {
            assert_eq!(TILE_MAX, 288.0);
            assert_eq!(columns_for(900.0, LARGE), 3);
            assert_eq!(tile_for(900.0, LARGE), 288.0);
            assert_eq!(tile_for(860.0, LARGE), 281.0);
        }
    }

    mod keeping_the_place {
        use super::*;

        fn nine() -> Vec<Section> {
            vec![flat(0, 9)]
        }
        // Two folders, so there is a header partway down to land on.
        fn folders() -> Vec<Section> {
            vec![folder(1, 0, 5), folder(2, 5, 4)]
        }

        #[test]
        fn the_pin_is_the_first_photo_of_the_row_at_the_top() {
            let rows = build_rows(&nine(), 3, MEDIUM);
            let pin = |offset| Pin {
                offset,
                header: false,
                into: 0.0,
            };
            assert_eq!(pin_at(&rows, 0.0), Some(pin(0)));
            assert_eq!(pin_at(&rows, tile_row(MEDIUM)), Some(pin(3)));
            assert_eq!(pin_at(&[], 0.0), None);
        }

        #[test]
        fn it_names_the_photo_not_the_pixel_anywhere_within_a_row() {
            let rows = build_rows(&nine(), 3, MEDIUM);
            let row = tile_row(MEDIUM);
            assert_eq!(pin_at(&rows, 2.0 * row).unwrap().offset, 6);
            assert_eq!(pin_at(&rows, 2.0 * row + row - 1.0).unwrap().offset, 6);
        }

        // The pin is spent on every frame of a resize: one that put the row's top at the
        // top of the grid would jump by up to a row on the first frame.
        #[test]
        fn it_comes_back_to_the_same_part_of_the_row_not_to_its_top() {
            let medium = build_rows(&nine(), 3, MEDIUM);
            let wider = build_rows(&nine(), 3, 200.0);
            let pin = pin_at(&medium, tile_row(MEDIUM) + tile_row(MEDIUM) / 4.0).unwrap();
            assert_eq!(
                pin,
                Pin {
                    offset: 3,
                    header: false,
                    into: 0.25
                }
            );
            assert_eq!(
                pin_top(&wider, pin),
                Some(tile_row(200.0) + tile_row(200.0) / 4.0)
            );
        }

        // The pin is read from the layout the user was looking at and spent in the one
        // that replaces it. Medium at three columns and small at five share no row tops.
        #[test]
        fn it_finds_its_row_in_a_layout_it_was_not_taken_in() {
            let medium = build_rows(&folders(), 3, MEDIUM);
            let small = build_rows(&folders(), 5, SMALL);
            for top in [0.0, 40.0, 200.0, 500.0, total_height(&medium) - 1.0] {
                let pin = pin_at(&medium, top).unwrap();
                let back = pin_top(&small, pin).unwrap();
                let first = small[row_of_item(&small, pin.offset).unwrap()].first;
                assert_eq!(pin_at(&small, back).unwrap().offset, first, "from {top}");
            }
        }

        // At a section's header the eye is on that section's first photo: an answer with
        // the tile row above would scroll the user back into a folder they had left. And it
        // comes back to the header itself, not to the row under it, which shares its offset.
        #[test]
        fn a_header_answers_with_the_section_it_heads_and_comes_back_to_it() {
            let medium = build_rows(&folders(), 3, MEDIUM);
            let small = build_rows(&folders(), 5, SMALL);
            let find = |rows: &[Row], kind| {
                *rows
                    .iter()
                    .find(|r| r.kind == kind && r.section == 1)
                    .unwrap()
            };
            let pin = pin_at(&medium, find(&medium, RowKind::Header).top).unwrap();
            assert_eq!(
                pin,
                Pin {
                    offset: 5,
                    header: true,
                    into: 0.0
                }
            );
            assert_eq!(
                pin_top(&small, pin),
                Some(find(&small, RowKind::Header).top)
            );
            // The row under it is another place, a header's height further down.
            let under = Pin {
                header: false,
                ..pin
            };
            assert_eq!(
                pin_top(&small, under),
                Some(find(&small, RowKind::Tiles).top)
            );
        }

        #[test]
        fn the_gap_under_a_folder_is_the_end_of_its_last_row() {
            let medium = build_rows(&folders(), 3, MEDIUM);
            let last = *medium
                .iter()
                .rfind(|r| r.kind == RowKind::Tiles && r.section == 0)
                .unwrap();
            assert_eq!(
                pin_at(&medium, last.top + last.height + 10.0),
                Some(Pin {
                    offset: last.first,
                    header: false,
                    into: 1.0
                })
            );
        }

        #[test]
        fn a_pin_whose_photo_is_gone_has_nowhere_to_come_back_to() {
            let rows = build_rows(&folders(), 3, MEDIUM);
            let gone = Pin {
                offset: 99,
                header: false,
                into: 0.0,
            };
            assert_eq!(pin_top(&rows, gone), None);
        }
    }

    mod the_pinned_header {
        use super::*;

        // Two folders at two columns of medium tiles: header 0, rows at 32, 200, 368; then
        // the section gap, header at 560, rows at 592 and 760.
        fn rows() -> Vec<Row> {
            build_rows(&two_folders(), 2, MEDIUM)
        }
        const SECOND: f64 = 560.0;

        fn pinned(top: f64) -> Option<PinnedHeader> {
            let rows = rows();
            pinned_header(&rows, &header_rows(&rows), top)
        }
        fn at(section: usize, y: f64) -> Option<PinnedHeader> {
            Some(PinnedHeader { section, y })
        }

        #[test]
        fn the_header_rows_in_order() {
            let rows = rows();
            let tops: Vec<_> = header_rows(&rows)
                .iter()
                .map(|&i| (rows[i].section, rows[i].top))
                .collect();
            assert_eq!(tops, [(0, 0.0), (1, SECOND)]);
            assert!(header_rows(&build_rows(&[flat(0, 5)], 2, MEDIUM)).is_empty());
        }

        // The real header is there, exactly where the pinned one would be drawn.
        #[test]
        fn nothing_is_pinned_while_the_header_itself_is_at_the_top() {
            assert_eq!(pinned(0.0), None);
            assert_eq!(pinned(SECOND), None);
        }

        #[test]
        fn the_header_of_the_section_the_top_is_inside_is_pinned() {
            assert_eq!(pinned(1.0), at(0, 0.0));
            assert_eq!(pinned(300.0), at(0, 0.0));
            assert_eq!(pinned(SECOND + 1.0), at(1, 0.0));
            assert_eq!(pinned(900.0), at(1, 0.0));
        }

        // In the space between two folders it is still the folder above the eye is leaving.
        #[test]
        fn the_folder_above_stays_pinned_through_the_gap_under_its_last_row() {
            let last_row_end = SECOND - SECTION_GAP;
            assert_eq!(pinned(last_row_end + 1.0).unwrap().section, 0);
        }

        // The pinned header's bottom edge rides on the arriving header's top.
        #[test]
        fn the_next_header_pushes_it_up_as_it_arrives() {
            assert_eq!(pinned(SECOND - HEADER), at(0, 0.0));
            assert_eq!(pinned(SECOND - HEADER + 10.0), at(0, -10.0));
            assert_eq!(pinned(SECOND - 1.0), at(0, -(HEADER - 1.0)));
        }

        #[test]
        fn nothing_is_pinned_without_headers_or_without_rows() {
            let flat = build_rows(&[flat(0, 5), flat(5, 3)], 2, MEDIUM);
            assert_eq!(pinned_header(&flat, &header_rows(&flat), 300.0), None);
            assert_eq!(pinned_header(&[], &[], 0.0), None);
        }

        // A run with no header after one with: nothing above it is its header.
        #[test]
        fn nothing_is_pinned_over_a_run_that_has_no_header_of_its_own() {
            let mixed = build_rows(&[folder(1, 0, 5), flat(5, 3)], 2, MEDIUM);
            let headers = header_rows(&mixed);
            let run = mixed
                .iter()
                .find(|r| r.kind == RowKind::Tiles && r.section == 1)
                .unwrap();
            assert_eq!(pinned_header(&mixed, &headers, run.top + 10.0), None);
            assert_eq!(pinned_header(&mixed, &headers, 100.0), at(0, 0.0));
        }
    }
}
