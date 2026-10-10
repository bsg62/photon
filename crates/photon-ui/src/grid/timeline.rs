//! The year strip beside the grid. Pure geometry, like `layout.rs`: the view only wires the
//! pointer to these. `ui/src/lib/timeline.ts` in Rust.
//!
//! The strip is the grid's whole layout scaled to the strip's height, so a year takes the
//! share of the strip its photos take of the scroll, and a point on the strip is a place in
//! the grid. It reads each folder section's `taken_at_min` - the value the sidebar files
//! folders under years by - so the strip, the sidebar and the grid stay on one axis. A
//! period section carries its own year.

use super::layout::{Row, RowKind, last_index_at_or_before};
use crate::sidebar::folders::year_of;
use jiff::tz::TimeZone;
use photon_core::grid::Section;

/// The strip's width beside the grid.
pub const TIMELINE_WIDTH: f64 = 44.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct YearMark {
    pub year: i64,
    /// Where the header of the first section of this run of the year is, in the layout.
    pub top: f64,
}

/// One mark wherever the year changes from the section before.
///
/// Not one mark for each year there is: a view by folder runs newest year to oldest, but
/// a search places a folder by its oldest photo while `taken_at_min` is its oldest
/// *matching* one, so years can come back. A year that comes back gets a second mark,
/// which is the truth of what scrolling there shows.
pub fn year_marks(sections: &[Section], rows: &[Row], zone: &TimeZone) -> Vec<YearMark> {
    let mut marks: Vec<YearMark> = Vec::new();
    for row in rows.iter().filter(|row| row.kind == RowKind::Header) {
        let Some(section) = sections.get(row.section) else {
            continue;
        };
        // A period section says its year itself, in the reading the engine grouped by; a
        // folder's is the year of its oldest photo, as the sidebar files it.
        let year = (section.period).map_or_else(
            || i64::from(year_of(section.taken_at_min, zone)),
            |period| period.year,
        );
        if marks.last().is_none_or(|last| last.year != year) {
            marks.push(YearMark { year, top: row.top });
        }
    }
    marks
}

/// The year showing at `y` in the layout, or none with no marks.
pub fn year_at(marks: &[YearMark], y: f64) -> Option<i64> {
    let at = last_index_at_or_before(marks, y, |mark| mark.top);
    marks.get(at).map(|mark| mark.year)
}

/// Where a place in the layout is on a strip `strip` tall, for a layout `total` tall.
pub fn strip_y(top: f64, total: f64, strip: f64) -> f64 {
    if total > 0.0 {
        top / total * strip
    } else {
        0.0
    }
}

/// The marks whose year is printed: greedily from the top, each at least `min_gap` down
/// the strip from the last one kept. A decade of small years would otherwise print as one
/// smear; the ones passed over are still reachable, the year under the pointer being read
/// from `year_at`.
pub fn labelled_marks(marks: &[YearMark], total: f64, strip: f64, min_gap: f64) -> Vec<YearMark> {
    let mut kept = Vec::new();
    let mut last = f64::NEG_INFINITY;
    for mark in marks {
        let y = strip_y(mark.top, total, strip);
        if y - last >= min_gap {
            kept.push(*mark);
            last = y;
        }
    }
    kept
}

/// The grid's position for a pointer `y` down the strip. A label stands at its year's own
/// `top` scaled, so a press on a label lands exactly on that year's first header - until
/// the grid's end holds it, which is what the last screenful of any scroll does.
pub fn scroll_top_for(y: f64, strip: f64, total: f64, viewport: f64) -> f64 {
    if strip <= 0.0 {
        return 0.0;
    }
    // Held to what the grid can scroll, which holds a pointer past either end of the
    // strip too.
    (y / strip * total).min(total - viewport).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::layout::build_rows;
    use photon_core::grid::Period;

    const MEDIUM: f64 = 160.0;

    /// Noon on 1 July, so no time zone can move it into a neighbouring year.
    fn mid(year: i16) -> i64 {
        jiff::civil::date(year, 7, 1)
            .at(12, 0, 0, 0)
            .to_zoned(TimeZone::UTC)
            .unwrap()
            .timestamp()
            .as_second()
    }

    fn section(folder_id: i64, offset: usize, count: usize, year: i16) -> Section {
        Section {
            folder_id: Some(folder_id),
            offset,
            count,
            taken_at_min: mid(year),
            period: None,
        }
    }

    fn years(marks: &[YearMark]) -> Vec<i64> {
        marks.iter().map(|mark| mark.year).collect()
    }

    // `taken_at_min` says another year on purpose: the period is what the header says.
    #[test]
    fn a_period_sections_own_year_is_read_not_the_instant_of_its_oldest_photo() {
        let period = |year, offset| Section {
            folder_id: None,
            offset,
            count: 1,
            taken_at_min: mid(2030),
            period: Some(Period {
                year,
                month: Some(12),
                day: None,
            }),
        };
        let sections = [period(2026, 0), period(2025, 1), period(2025, 2)];
        let rows = build_rows(&sections, 4, MEDIUM);
        assert_eq!(
            years(&year_marks(&sections, &rows, &TimeZone::UTC)),
            [2026, 2025]
        );
    }

    #[test]
    fn the_first_header_of_each_run_of_a_year_is_marked() {
        let sections = [
            section(1, 0, 3, 2024),
            section(2, 3, 2, 2024),
            section(3, 5, 4, 2019),
        ];
        let rows = build_rows(&sections, 2, MEDIUM);
        // 2024's header is at the top. Folder 1 is two rows of tiles, folder 2 a header
        // and one: 2019's header follows them.
        let top_2019 = (rows.iter())
            .find(|row| row.kind == RowKind::Header && row.section == 2)
            .unwrap()
            .top;
        assert!(top_2019 > 0.0);
        assert_eq!(
            year_marks(&sections, &rows, &TimeZone::UTC),
            [
                YearMark {
                    year: 2024,
                    top: 0.0
                },
                YearMark {
                    year: 2019,
                    top: top_2019
                },
            ]
        );
    }

    #[test]
    fn a_year_that_comes_back_is_marked_again_as_a_search_can_make_it() {
        let sections = [
            section(1, 0, 1, 2020),
            section(2, 1, 1, 2024),
            section(3, 2, 1, 2020),
        ];
        let rows = build_rows(&sections, 4, MEDIUM);
        assert_eq!(
            years(&year_marks(&sections, &rows, &TimeZone::UTC)),
            [2020, 2024, 2020]
        );
    }

    #[test]
    fn without_headers_there_is_nothing_to_mark() {
        let sections = [Section {
            folder_id: None,
            ..section(1, 0, 3, 2024)
        }];
        let rows = build_rows(&sections, 2, MEDIUM);
        assert_eq!(year_marks(&sections, &rows, &TimeZone::UTC), []);
    }

    // The sidebar files a folder under the year its oldest photo was taken in where the
    // viewer is: the strip reads the same instant the same way.
    #[test]
    fn a_folders_year_is_read_in_the_viewers_zone() {
        let new_year = mid(2026) - 181 * 86_400 - 12 * 3600 + 1800;
        let sections = [Section {
            taken_at_min: new_year,
            ..section(1, 0, 1, 2026)
        }];
        let rows = build_rows(&sections, 4, MEDIUM);
        assert_eq!(years(&year_marks(&sections, &rows, &TimeZone::UTC)), [2026]);
        let west = TimeZone::fixed(jiff::tz::offset(-5));
        assert_eq!(years(&year_marks(&sections, &rows, &west)), [2025]);
    }

    #[test]
    fn the_year_at_a_place_is_that_of_the_last_mark_at_or_above_it() {
        let marks = [
            YearMark {
                year: 2024,
                top: 0.0,
            },
            YearMark {
                year: 2019,
                top: 500.0,
            },
        ];
        assert_eq!(year_at(&marks, 0.0), Some(2024));
        assert_eq!(year_at(&marks, 499.0), Some(2024));
        assert_eq!(year_at(&marks, 500.0), Some(2019));
        assert_eq!(year_at(&marks, 9999.0), Some(2019));
        assert_eq!(year_at(&[], 10.0), None);
    }

    #[test]
    fn a_label_that_would_print_over_the_one_before_it_is_dropped() {
        let mark = |year, top| YearMark { year, top };
        let marks = [
            mark(2024, 0.0),
            mark(2023, 50.0), // 5 down a strip of 100: too close to 2024
            mark(2022, 300.0),
            mark(2021, 390.0), // 9 below 2022: too close
            mark(2020, 900.0),
        ];
        assert_eq!(
            years(&labelled_marks(&marks, 1000.0, 100.0, 14.0)),
            [2024, 2022, 2020]
        );
    }

    #[test]
    fn a_press_on_a_label_lands_exactly_on_that_year() {
        let (total, strip) = (10_000.0, 400.0);
        let y = strip_y(2_500.0, total, strip);
        assert_eq!(scroll_top_for(y, strip, total, 800.0), 2_500.0);
        assert_eq!(strip_y(2_500.0, 0.0, strip), 0.0);
    }

    #[test]
    fn a_place_on_the_strip_is_held_to_what_the_grid_can_scroll() {
        assert_eq!(scroll_top_for(-20.0, 400.0, 10_000.0, 800.0), 0.0);
        assert_eq!(scroll_top_for(400.0, 400.0, 10_000.0, 800.0), 9_200.0);
        assert_eq!(scroll_top_for(999.0, 400.0, 10_000.0, 800.0), 9_200.0);
        assert_eq!(scroll_top_for(100.0, 0.0, 10_000.0, 800.0), 0.0);
        // A layout shorter than the viewport cannot scroll at all.
        assert_eq!(scroll_top_for(200.0, 400.0, 500.0, 800.0), 0.0);
    }
}
