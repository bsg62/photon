//! What the three controls at the right of the top bar offer, and what a choice in one
//! makes of the sort: `SortControl.svelte`, `GroupControl.svelte`, `SizeControl.svelte` and
//! `grouping.ts`, without their markup.
//!
//! A control changes the sort it was given by replacing one field of it. Built from two
//! fields, the Svelte sort control once dropped the grouping.
//!
//! No egui here.

use photon_core::{
    library::GridTile,
    sort::{Grouping, Sort, SortKey},
};

/// The sort control's options.
pub const SORT_KEYS: [(SortKey, &str); 4] = [
    (SortKey::Date, "Date taken"),
    (SortKey::Modified, "Date modified"),
    (SortKey::Name, "Name"),
    (SortKey::Size, "Size"),
];

/// The grouping control's options. Each label says what it is on its own: a closed select
/// shows only its value, and a bare "Folder" beside "Date taken" does not.
pub const GROUPINGS: [(Grouping, &str); 5] = [
    (Grouping::Folder, "By folder"),
    (Grouping::Day, "By day"),
    (Grouping::Month, "By month"),
    (Grouping::Year, "By year"),
    (Grouping::None, "No grouping"),
];

/// The size control's segments.
pub const SIZES: [(GridTile, &str); 3] = [
    (GridTile::Small, "Small"),
    (GridTile::Medium, "Medium"),
    (GridTile::Large, "Large"),
];

/// What the grouping control says under the pointer while it is dimmed.
pub const GROUPING_IDLE: &str = "Grouping applies when sorted by date taken";

/// Whether the grouping decides anything: only by date. By name, size or modification
/// time every photo is sorted together and the stored grouping waits for the sort to come
/// back - which is why the control is dimmed there and not removed: removed, the bar's
/// controls would shift with every change of sort, and the choice it still holds would be
/// out of sight.
pub fn grouping_applies(sort: Sort) -> bool {
    sort.key == SortKey::Date
}

/// Which of `options` holds `value`.
pub fn held<T: PartialEq + Copy>(options: &[(T, &str)], value: T) -> Option<usize> {
    options.iter().position(|(option, _)| *option == value)
}

/// `sort` by another key: its direction and its grouping are kept.
pub fn with_key(sort: Sort, index: usize) -> Sort {
    Sort {
        key: SORT_KEYS.get(index).map_or(sort.key, |(key, _)| *key),
        ..sort
    }
}

/// `sort` under another grouping.
pub fn with_grouping(sort: Sort, index: usize) -> Sort {
    Sort {
        group: GROUPINGS.get(index).map_or(sort.group, |(group, _)| *group),
        ..sort
    }
}

/// `sort` turned over.
pub fn reversed(sort: Sort) -> Sort {
    Sort {
        reverse: !sort.reverse,
        ..sort
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BY_MONTH_REVERSED: Sort = Sort {
        key: SortKey::Date,
        reverse: true,
        group: Grouping::Month,
    };

    #[test]
    fn a_control_changes_one_field_of_the_sort_and_keeps_the_others() {
        assert_eq!(
            with_key(BY_MONTH_REVERSED, 2),
            Sort {
                key: SortKey::Name,
                ..BY_MONTH_REVERSED
            }
        );
        assert_eq!(
            with_grouping(BY_MONTH_REVERSED, 3),
            Sort {
                group: Grouping::Year,
                ..BY_MONTH_REVERSED
            }
        );
        assert_eq!(
            reversed(BY_MONTH_REVERSED),
            Sort {
                reverse: false,
                ..BY_MONTH_REVERSED
            }
        );
        // An option that is not there changes nothing.
        assert_eq!(with_key(BY_MONTH_REVERSED, 9), BY_MONTH_REVERSED);
        assert_eq!(with_grouping(BY_MONTH_REVERSED, 9), BY_MONTH_REVERSED);
    }

    #[test]
    fn each_control_holds_the_option_of_the_sort_it_is_given() {
        assert_eq!(held(&SORT_KEYS, SortKey::Date), Some(0));
        assert_eq!(held(&SORT_KEYS, SortKey::Size), Some(3));
        assert_eq!(held(&GROUPINGS, Grouping::Month), Some(2));
        assert_eq!(held(&GROUPINGS, Grouping::None), Some(4));
        assert_eq!(held(&SIZES, GridTile::Large), Some(2));
        // Every key and every grouping has an option, and chosen it is the one held.
        for (index, (key, _)) in SORT_KEYS.iter().enumerate() {
            assert_eq!(with_key(Sort::default(), index).key, *key);
        }
        for (index, (group, _)) in GROUPINGS.iter().enumerate() {
            assert_eq!(with_grouping(Sort::default(), index).group, *group);
        }
    }

    #[test]
    fn the_grouping_decides_something_only_by_date() {
        assert!(grouping_applies(Sort::default()));
        assert!(grouping_applies(BY_MONTH_REVERSED));
        for key in [SortKey::Modified, SortKey::Name, SortKey::Size] {
            assert!(!grouping_applies(Sort {
                key,
                ..BY_MONTH_REVERSED
            }));
        }
    }

    // The words are the Svelte controls', until the switch-over.
    #[test]
    fn the_options_are_worded_as_the_svelte_controls_word_them() {
        let sort = include_str!("../../../ui/src/components/SortControl.svelte");
        for (_, label) in SORT_KEYS {
            assert!(sort.contains(&format!("label: '{label}'")), "{label}");
        }
        let grouping = include_str!("../../../ui/src/lib/grouping.ts");
        for (_, label) in GROUPINGS {
            assert!(grouping.contains(&format!("label: '{label}'")), "{label}");
        }
        let size = include_str!("../../../ui/src/components/SizeControl.svelte");
        for (_, label) in SIZES {
            assert!(size.contains(&format!("label: '{label}'")), "{label}");
        }
        let group = include_str!("../../../ui/src/components/GroupControl.svelte");
        assert!(group.contains(GROUPING_IDLE));
    }
}
