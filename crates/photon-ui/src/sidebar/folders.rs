//! The folders the sidebar lists, and their order: `ui/src/lib/folders.ts`'s list, case for
//! case, until the switch-over deletes that file.
//!
//! No egui here.

use crate::grid::labels::folder_label;
use jiff::{Timestamp, tz::TimeZone};
use photon_core::{
    grid::FolderTally,
    library::Folder,
    sort::{Sort, SortKey, natural_cmp},
};
use std::{cmp::Ordering, collections::HashMap};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderRow {
    pub folder_id: i64,
    pub name: String,
    pub count: usize,
    pub year: i16,
    /// Capture time of the folder's oldest photo, in seconds. Decides both the year group
    /// and the order within it.
    pub taken_at_min: i64,
    /// The folder's photos in the view, in bytes.
    pub bytes: i64,
    /// The newest modification time among them, in milliseconds.
    pub modified_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YearGroup {
    /// `None` for the one headerless group a sort other than date lists every folder in.
    pub year: Option<i16>,
    pub rows: Vec<FolderRow>,
}

/// The year `taken_at_min` falls in, read in the viewer's zone rather than UTC: a person
/// means their own new year, so a photo taken at 23:00 on 31 December belongs to the year
/// they experienced. The grid's header reads the month the same way (`folder_summary`), so
/// the two name one instant alike.
pub fn year_of(taken_at_min: i64, zone: &TimeZone) -> i16 {
    // A date no calendar holds is filed where the epoch is, not left out.
    Timestamp::from_second(taken_at_min)
        .map_or(1970, |instant| instant.to_zoned(zone.clone()).year())
}

/// One row per folder that holds photos in the view.
///
/// Drawn from the index's tallies rather than the folder list, which is what leaves out
/// the empty folders between: the folder table holds every directory on the way to a
/// photo. One tally per folder however its photos are arranged.
pub fn folder_rows(
    tallies: &[FolderTally],
    folders: &HashMap<i64, Folder>,
    zone: &TimeZone,
) -> Vec<FolderRow> {
    tallies
        .iter()
        .map(|tally| FolderRow {
            folder_id: tally.folder_id,
            // A tally implies a photo, which implies a folder - but a tally can arrive
            // before the folder list has been read again, and a blank name beats losing
            // the row.
            name: folders
                .get(&tally.folder_id)
                .map(|folder| folder_label(folder).to_owned())
                .unwrap_or_default(),
            count: tally.count,
            year: year_of(tally.taken_at_min, zone),
            taken_at_min: tally.taken_at_min,
            bytes: tally.bytes,
            modified_ms: tally.modified_ms,
        })
        .collect()
}

/// Years newest first, and within a year the folder whose oldest photo is newest; all of
/// it turned over when `reverse`.
pub fn group_by_year(rows: Vec<FolderRow>, reverse: bool) -> Vec<YearGroup> {
    let turned = |order: Ordering| if reverse { order.reverse() } else { order };
    let mut rows = rows;
    // Stable, so folders whose oldest photos share one second keep the order the grid
    // first reaches them in - reversed or not, since the comparison is what is turned
    // over and not the list.
    rows.sort_by(|a, b| {
        turned(b.year.cmp(&a.year)).then_with(|| turned(b.taken_at_min.cmp(&a.taken_at_min)))
    });
    let mut groups: Vec<YearGroup> = Vec::new();
    for row in rows {
        match groups.last_mut() {
            Some(group) if group.year == Some(row.year) => group.rows.push(row),
            _ => groups.push(YearGroup {
                year: Some(row.year),
                rows: vec![row],
            }),
        }
    }
    groups
}

/// The sidebar's folder list under the user's sort.
///
/// By date it is `group_by_year`, and reversed it is that list turned over - years oldest
/// first and each year's folders oldest first - which, grouped by folder, is the order the
/// reversed grid reaches them in. By any other key the grid is flat and has no folder order
/// to follow, so the list answers the key's question about folders instead (the biggest,
/// the most recently touched) and drops the year headings, which would split a list sorted
/// by name into pieces sorted by something else. Ties keep the order the grid first
/// reaches each folder.
///
/// Names are ordered as the grid's own name sort orders file names (`natural_cmp` over
/// lower case), so the list and the grid agree beyond ASCII, where the Svelte list used the
/// browser's collation and did not.
pub fn arrange_folders(rows: Vec<FolderRow>, sort: Sort) -> Vec<YearGroup> {
    let order: fn(&(String, FolderRow), &(String, FolderRow)) -> Ordering = match sort.key {
        SortKey::Date => return group_by_year(rows, sort.reverse),
        SortKey::Name => |a, b| natural_cmp(&a.0, &b.0),
        SortKey::Size => |a, b| b.1.bytes.cmp(&a.1.bytes),
        SortKey::Modified => |a, b| b.1.modified_ms.cmp(&a.1.modified_ms),
    };
    if rows.is_empty() {
        return Vec::new();
    }
    let mut keyed: Vec<(String, FolderRow)> = rows
        .into_iter()
        .map(|row| (row.name.to_lowercase(), row))
        .collect();
    // Reversed by the comparison rather than by turning the result over, which would
    // flip every tie against the grid's order.
    keyed.sort_by(|a, b| {
        let order = order(a, b);
        if sort.reverse { order.reverse() } else { order }
    });
    vec![YearGroup {
        year: None,
        rows: keyed.into_iter().map(|(_, row)| row).collect(),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use photon_core::sort::Grouping;

    /// Noon UTC on the day, in seconds.
    fn at(year: i16, month: i8, day: i8) -> i64 {
        jiff::civil::date(year, month, day)
            .at(12, 0, 0, 0)
            .to_zoned(TimeZone::UTC)
            .unwrap()
            .timestamp()
            .as_second()
    }

    fn folder(id: i64, name: &str) -> Folder {
        Folder {
            id,
            watched_id: 1,
            parent_id: (id != 1).then_some(1),
            path: format!("/photos/{name}"),
            name: name.to_owned(),
            hidden: false,
            alias: None,
        }
    }

    fn folders() -> HashMap<i64, Folder> {
        [
            folder(1, "photos"),
            folder(2, "rome"),
            folder(3, "oslo"),
            folder(4, "old"),
        ]
        .into_iter()
        .map(|folder| (folder.id, folder))
        .collect()
    }

    fn tallies() -> Vec<FolderTally> {
        let tally = |folder_id, count, taken_at_min, bytes, modified_ms| FolderTally {
            folder_id,
            count,
            taken_at_min,
            bytes,
            modified_ms,
        };
        vec![
            tally(2, 12, at(2024, 6, 1), 500, 3_000),
            tally(3, 3, at(2024, 11, 20), 900, 1_000),
            tally(4, 40, at(2019, 2, 2), 100, 2_000),
        ]
    }

    fn rows() -> Vec<FolderRow> {
        folder_rows(&tallies(), &folders(), &TimeZone::UTC)
    }

    fn sort(key: SortKey, reverse: bool) -> Sort {
        Sort {
            key,
            reverse,
            group: Grouping::Folder,
        }
    }

    fn names(groups: &[YearGroup]) -> Vec<Vec<&str>> {
        groups
            .iter()
            .map(|group| group.rows.iter().map(|row| row.name.as_str()).collect())
            .collect()
    }

    fn row(folder_id: i64, name: &str, bytes: i64) -> FolderRow {
        FolderRow {
            folder_id,
            name: name.to_owned(),
            count: 1,
            year: 2024,
            taken_at_min: 0,
            bytes,
            modified_ms: 0,
        }
    }

    #[test]
    fn each_folder_that_has_photos_is_named_and_carries_its_count() {
        let rows = rows();
        assert_eq!(
            rows[0],
            FolderRow {
                folder_id: 2,
                name: "rome".to_owned(),
                count: 12,
                year: 2024,
                taken_at_min: at(2024, 6, 1),
                bytes: 500,
                modified_ms: 3_000,
            }
        );
        let listed: Vec<_> = rows
            .iter()
            .map(|row| (row.name.as_str(), row.year))
            .collect();
        assert_eq!(listed, [("rome", 2024), ("oslo", 2024), ("old", 2019)]);
        // Folder 1 holds no photos of its own - it has no tally - so it is not listed,
        // which is the whole point of listing tallies rather than the folder table.
        assert!(rows.iter().all(|row| row.name != "photos"));
        assert_eq!(folder_rows(&[], &folders(), &TimeZone::UTC), []);
    }

    #[test]
    fn a_folder_is_shown_by_its_alias_in_place_of_its_directory_name() {
        let mut folders = folders();
        folders.get_mut(&3).unwrap().alias = Some("Aarhus trip".to_owned());
        let rows = folder_rows(&tallies(), &folders, &TimeZone::UTC);
        let listed: Vec<_> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(listed, ["rome", "Aarhus trip", "old"]);
    }

    // A tally can arrive before the folder list has been read again. There is no path to
    // fall back to, so the name is blank - which beats losing the row.
    #[test]
    fn a_tally_whose_folder_has_not_arrived_yet_is_still_a_row() {
        let tally = FolderTally {
            folder_id: 99,
            count: 1,
            taken_at_min: at(2024, 1, 1),
            bytes: 1,
            modified_ms: 1,
        };
        let rows = folder_rows(&[tally], &folders(), &TimeZone::UTC);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].name.as_str(), rows[0].year), ("", 2024));
    }

    // 2026-01-01 00:30 UTC: the new year in UTC, still the old one five hours west.
    #[test]
    fn a_folders_year_is_read_in_the_viewers_zone() {
        let new_year = at(2026, 1, 1) - 12 * 3600 + 30 * 60;
        assert_eq!(year_of(new_year, &TimeZone::UTC), 2026);
        assert_eq!(
            year_of(new_year, &TimeZone::fixed(jiff::tz::offset(-5))),
            2025
        );
        // A date no calendar holds is filed somewhere, not left out.
        assert_eq!(year_of(i64::MAX, &TimeZone::UTC), 1970);
    }

    #[test]
    fn folders_are_grouped_by_year_newest_year_first() {
        let groups = group_by_year(rows(), false);
        let years: Vec<_> = groups.iter().map(|group| group.year).collect();
        assert_eq!(years, [Some(2024), Some(2019)]);
        // oslo's oldest photo is from November and rome's from June, so oslo leads
        // although rome comes first in the grid's order.
        assert_eq!(names(&groups), [vec!["oslo", "rome"], vec!["old"]]);
        assert_eq!(group_by_year(Vec::new(), false), []);
    }

    #[test]
    fn by_date_the_years_are_kept_and_turned_over_when_reversed() {
        assert_eq!(
            arrange_folders(rows(), sort(SortKey::Date, false)),
            group_by_year(rows(), false)
        );
        let reversed = arrange_folders(rows(), sort(SortKey::Date, true));
        let years: Vec<_> = reversed.iter().map(|group| group.year).collect();
        assert_eq!(years, [Some(2019), Some(2024)]);
        assert_eq!(names(&reversed), [vec!["old"], vec!["rome", "oslo"]]);
    }

    #[test]
    fn by_size_and_by_modified_every_folder_is_under_one_headerless_group() {
        let by_size = arrange_folders(rows(), sort(SortKey::Size, false));
        assert_eq!(by_size.len(), 1);
        assert_eq!(by_size[0].year, None);
        assert_eq!(names(&by_size), [["oslo", "rome", "old"]]);
        assert_eq!(
            names(&arrange_folders(rows(), sort(SortKey::Size, true))),
            [["old", "rome", "oslo"]]
        );
        assert_eq!(
            names(&arrange_folders(rows(), sort(SortKey::Modified, false))),
            [["rome", "old", "oslo"]]
        );
    }

    #[test]
    fn an_aliased_folder_is_sorted_by_its_alias() {
        let mut folders = folders();
        folders.get_mut(&3).unwrap().alias = Some("Aarhus trip".to_owned());
        let rows = folder_rows(&tallies(), &folders, &TimeZone::UTC);
        assert_eq!(
            names(&arrange_folders(rows, sort(SortKey::Name, false))),
            [["Aarhus trip", "old", "rome"]]
        );
    }

    #[test]
    fn names_are_sorted_ignoring_case_and_reading_numbers() {
        let rows = || {
            vec![
                row(1, "Trip 10", 0),
                row(2, "beach", 0),
                row(3, "trip 2", 0),
                row(4, "Attic", 0),
            ]
        };
        assert_eq!(
            names(&arrange_folders(rows(), sort(SortKey::Name, false))),
            [["Attic", "beach", "trip 2", "Trip 10"]]
        );
        assert_eq!(
            names(&arrange_folders(rows(), sort(SortKey::Name, true))),
            [["Trip 10", "trip 2", "beach", "Attic"]]
        );
    }

    // Reversed, the tallies already arrive in the reversed grid's order; ties must keep it.
    #[test]
    fn folders_that_tie_keep_the_grids_order_reversed_or_not() {
        let rows = || vec![row(1, "c", 7), row(2, "a", 7), row(3, "b", 7)];
        for (key, reverse) in [
            (SortKey::Size, false),
            (SortKey::Size, true),
            (SortKey::Date, false),
            (SortKey::Date, true),
        ] {
            assert_eq!(
                names(&arrange_folders(rows(), sort(key, reverse))),
                [["c", "a", "b"]],
                "{key:?} reversed {reverse}"
            );
        }
    }

    #[test]
    fn no_folders_are_no_group_and_not_an_empty_one() {
        assert_eq!(arrange_folders(Vec::new(), sort(SortKey::Name, false)), []);
    }
}
