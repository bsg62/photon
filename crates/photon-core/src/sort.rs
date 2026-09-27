//! The order the grid and the sidebar list photos in, chosen by the user.
//!
//! Date is the library's own order - folder first, by each folder's oldest photo, as
//! `GRID_ORDER` spells it - and the one that keeps the folder sections and the timeline. The
//! other keys are about the photos rather than the folders, so they sort every photo in the
//! view together and lay the grid out flat: sorted by size within each folder, "the largest
//! photos" would still mean "the largest photos of the newest folder".
//!
//! The sort runs here, in Rust, over the rows a view's query already returned, rather than
//! as one more `ORDER BY` per key: the name has to compare case-insensitively, which SQL
//! cannot do here beyond ASCII (see `CLAUDE.md`), and one sort over rows already in memory
//! is cheaper than teaching every view's query a second shape.

use crate::grid::{GridEntry, GridView, Layout};
use serde::{Deserialize, Serialize};
use std::cmp::{Ordering, Reverse};

/// What the grid is sorted by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortKey {
    /// The capture date, in the view's own order: folders newest first, each read oldest to
    /// newest; Recent newest first.
    #[default]
    Date,
    /// The file's modification time, newest first.
    Modified,
    /// The file name, A to Z, ignoring case and reading digit runs as numbers.
    Name,
    /// The file's size, largest first.
    Size,
}

impl SortKey {
    fn as_str(self) -> &'static str {
        match self {
            Self::Date => "date",
            Self::Modified => "modified",
            Self::Name => "name",
            Self::Size => "size",
        }
    }

    fn parse(stored: &str) -> Option<Self> {
        match stored {
            "date" => Some(Self::Date),
            "modified" => Some(Self::Modified),
            "name" => Some(Self::Name),
            "size" => Some(Self::Size),
            _ => None,
        }
    }
}

/// A key and whether it runs backwards. `reverse` turns the whole list upside down, the
/// Date order included - folders oldest first, each read newest to oldest - so it means one
/// thing for every key and the folder runs stay contiguous for the sections.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sort {
    pub key: SortKey,
    pub reverse: bool,
}

impl Sort {
    /// How `view` is laid out under this sort: the view's own layout by date, flat by any
    /// other key, whose runs of one folder are an accident of the key rather than sections.
    pub fn layout(self, view: GridView) -> Layout {
        match self.key {
            SortKey::Date => view.layout(),
            _ => Layout::Flat,
        }
    }

    /// The form stored in the settings table: the key's name, with a leading `-` when
    /// reversed.
    pub fn to_setting(self) -> String {
        let sign = if self.reverse { "-" } else { "" };
        format!("{sign}{}", self.key.as_str())
    }

    /// Reads [`to_setting`](Self::to_setting)'s form. Anything else - a key a newer photon
    /// added, a hand-edited row - falls back to the default rather than failing to open.
    pub fn from_setting(stored: &str) -> Self {
        let (reverse, key) = match stored.strip_prefix('-') {
            Some(key) => (true, key),
            None => (false, stored),
        };
        SortKey::parse(key).map_or_else(Self::default, |key| Self { key, reverse })
    }

    /// Puts `entries`, which arrive in the view's own order, into this sort's order. `name`
    /// answers an entry's file name by id and is only asked when sorting by name.
    ///
    /// The sort is stable, so photos that tie - the same size, the same second - keep the
    /// view's own order between them rather than an arbitrary one that could change between
    /// rebuilds and shuffle the grid under the user.
    pub fn arrange<'a>(self, entries: &mut Vec<GridEntry>, name: impl Fn(i64) -> &'a str) {
        match self.key {
            SortKey::Date => {}
            SortKey::Modified => entries.sort_by_key(|e| Reverse(e.mtime_ms)),
            SortKey::Size => entries.sort_by_key(|e| Reverse(e.size)),
            SortKey::Name => {
                // Lower-cased once per entry rather than once per comparison.
                let mut keyed: Vec<(String, GridEntry)> = entries
                    .drain(..)
                    .map(|entry| (name(entry.id).to_lowercase(), entry))
                    .collect();
                keyed.sort_by(|a, b| natural_cmp(&a.0, &b.0));
                entries.extend(keyed.into_iter().map(|(_, entry)| entry));
            }
        }
        if self.reverse {
            entries.reverse();
        }
    }
}

/// Compares two strings with every run of ASCII digits read as a number, so `IMG_2` sorts
/// before `IMG_10` the way a person reads them. Case is the caller's business.
///
/// Numbers are compared by value without parsing them, so a run longer than any integer
/// type still compares: leading zeros are skipped, then the longer run is the larger, then
/// the digits decide. Runs of equal value but different zero padding (`7` and `007`) compare
/// equal here; the stable sort leaves them in the view's order.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a.first(), b.first()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (x_run, x_rest) = split_digits(a);
                let (y_run, y_rest) = split_digits(b);
                let (x_run, y_run) = (trim_zeros(x_run), trim_zeros(y_run));
                let by_value = x_run.len().cmp(&y_run.len()).then_with(|| x_run.cmp(y_run));
                if by_value != Ordering::Equal {
                    return by_value;
                }
                (a, b) = (x_rest, y_rest);
            }
            (Some(_), Some(_)) => {
                // Byte-wise outside digit runs: equal to comparing the UTF-8 strings by code
                // point, and a digit is never part of a multi-byte character.
                let x_end = a.iter().position(u8::is_ascii_digit).unwrap_or(a.len());
                let y_end = b.iter().position(u8::is_ascii_digit).unwrap_or(b.len());
                let (x_text, y_text) = (&a[..x_end], &b[..y_end]);
                // Compared up to the shorter one's end: where one text run is a prefix of the
                // other, what follows it - a digit, or the end - decides on the next turn.
                let common = x_text.len().min(y_text.len());
                let by_text = x_text[..common].cmp(&y_text[..common]);
                if by_text != Ordering::Equal {
                    return by_text;
                }
                (a, b) = (&a[common..], &b[common..]);
                if common == 0 {
                    // One side is at a digit (an empty text run) and the other at text: the
                    // two bytes decide, as they would in plain byte order - so `photo.jpg`
                    // comes before `photo1.jpg` (`.` is below `1`) and `photo1` before
                    // `photob`. The two differ, since only one of them is a digit.
                    return a[0].cmp(&b[0]);
                }
            }
        }
    }
}

fn split_digits(s: &[u8]) -> (&[u8], &[u8]) {
    let end = s
        .iter()
        .position(|c| !c.is_ascii_digit())
        .unwrap_or(s.len());
    s.split_at(end)
}

fn trim_zeros(run: &[u8]) -> &[u8] {
    let start = run.iter().position(|&c| c != b'0').unwrap_or(run.len());
    &run[start..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::MediaKind;

    fn entry(id: i64, size: i64, mtime_ms: i64) -> GridEntry {
        GridEntry {
            id,
            folder_id: 1,
            taken_at: id,
            aspect: 1.0,
            kind: MediaKind::Image,
            duration_ms: None,
            starred: false,
            has_copies: false,
            thumb_key: 0,
            size,
            mtime_ms,
        }
    }

    fn ids(entries: &[GridEntry]) -> Vec<i64> {
        entries.iter().map(|e| e.id).collect()
    }

    fn names(id: i64) -> &'static str {
        match id {
            1 => "IMG_10.jpg",
            2 => "img_2.jpg",
            3 => "Beach.jpg",
            _ => "IMG_2.jpg",
        }
    }

    #[test]
    fn digit_runs_compare_as_numbers() {
        assert_eq!(natural_cmp("img_2.jpg", "img_10.jpg"), Ordering::Less);
        assert_eq!(natural_cmp("img_10.jpg", "img_9.jpg"), Ordering::Greater);
        assert_eq!(natural_cmp("img_007", "img_7"), Ordering::Equal);
        assert_eq!(natural_cmp("a", "a1"), Ordering::Less);
        assert_eq!(natural_cmp("a1", "ab"), Ordering::Less);
        // Punctuation below `0` sorts ahead of a digit, as in byte order.
        assert_eq!(natural_cmp("photo.jpg", "photo1.jpg"), Ordering::Less);
        assert_eq!(natural_cmp("photo1.jpg", "photo.jpg"), Ordering::Greater);
        assert_eq!(natural_cmp("a-1", "a1"), Ordering::Less);
        assert_eq!(natural_cmp("ab", "a1"), Ordering::Greater);
        assert_eq!(natural_cmp("2024 trip", "2024"), Ordering::Greater);
        assert_eq!(natural_cmp("beach", "beach"), Ordering::Equal);
        assert_eq!(natural_cmp("beach", "beaches"), Ordering::Less);
        // Longer than any integer type, and still by value.
        assert_eq!(
            natural_cmp("x99999999999999999999999", "x100000000000000000000000"),
            Ordering::Less
        );
    }

    #[test]
    fn name_ignores_case_and_reads_numbers() {
        let mut entries = vec![entry(1, 0, 0), entry(2, 0, 0), entry(3, 0, 0)];
        let sort = Sort {
            key: SortKey::Name,
            reverse: false,
        };
        sort.arrange(&mut entries, names);
        // Byte order would put "Beach" and "IMG_10" before "img_2".
        assert_eq!(ids(&entries), [3, 2, 1]);
    }

    #[test]
    fn size_and_modified_run_largest_and_newest_first() {
        let fresh = || vec![entry(1, 10, 300), entry(2, 30, 100), entry(3, 20, 200)];
        let mut by_size = fresh();
        Sort {
            key: SortKey::Size,
            reverse: false,
        }
        .arrange(&mut by_size, names);
        assert_eq!(ids(&by_size), [2, 3, 1]);

        let mut by_modified = fresh();
        Sort {
            key: SortKey::Modified,
            reverse: false,
        }
        .arrange(&mut by_modified, names);
        assert_eq!(ids(&by_modified), [1, 3, 2]);
    }

    #[test]
    fn ties_keep_the_views_own_order() {
        let mut entries = vec![
            entry(1, 5, 0),
            entry(2, 9, 0),
            entry(3, 5, 0),
            entry(4, 5, 0),
        ];
        Sort {
            key: SortKey::Size,
            reverse: false,
        }
        .arrange(&mut entries, names);
        assert_eq!(ids(&entries), [2, 1, 3, 4]);
        // Names that differ only in case tie too, and keep the order they arrived in.
        let mut entries = vec![entry(4, 0, 0), entry(2, 0, 0)];
        Sort {
            key: SortKey::Name,
            reverse: false,
        }
        .arrange(&mut entries, names);
        assert_eq!(ids(&entries), [4, 2]);
        // Long enough that the standard library's sort leaves insertion sort, which is
        // stable by accident: every key sorted unstably passes the short cases above.
        for key in [SortKey::Size, SortKey::Modified, SortKey::Name] {
            let mut entries: Vec<GridEntry> =
                (0..200).map(|id| entry(id, id % 3, id % 3)).collect();
            Sort {
                key,
                reverse: false,
            }
            .arrange(&mut entries, |id| if id % 2 == 0 { "a" } else { "b" });
            for pair in entries.windows(2) {
                let same = match key {
                    SortKey::Name => pair[0].id % 2 == pair[1].id % 2,
                    _ => pair[0].size == pair[1].size,
                };
                if same {
                    assert!(
                        pair[0].id < pair[1].id,
                        "{key:?}: {} before {}",
                        pair[0].id,
                        pair[1].id
                    );
                }
            }
        }
    }

    #[test]
    fn date_leaves_the_views_order_and_reverse_turns_any_order_over() {
        let fresh = || vec![entry(1, 10, 0), entry(2, 30, 0), entry(3, 20, 0)];
        let mut by_date = fresh();
        Sort::default().arrange(&mut by_date, names);
        assert_eq!(ids(&by_date), [1, 2, 3]);

        let mut reversed = fresh();
        Sort {
            key: SortKey::Date,
            reverse: true,
        }
        .arrange(&mut reversed, names);
        assert_eq!(ids(&reversed), [3, 2, 1]);

        let mut smallest_first = fresh();
        Sort {
            key: SortKey::Size,
            reverse: true,
        }
        .arrange(&mut smallest_first, names);
        assert_eq!(ids(&smallest_first), [1, 3, 2]);
    }

    #[test]
    fn only_date_keeps_the_views_layout() {
        for key in [SortKey::Modified, SortKey::Name, SortKey::Size] {
            for reverse in [false, true] {
                let sort = Sort { key, reverse };
                assert_eq!(sort.layout(GridView::All), Layout::Flat, "{sort:?}");
            }
        }
        let date = Sort::default();
        assert_eq!(date.layout(GridView::All), Layout::Folders);
        let reversed = Sort {
            key: SortKey::Date,
            reverse: true,
        };
        assert_eq!(reversed.layout(GridView::Starred), Layout::Folders);
        assert_eq!(date.layout(GridView::Recent), Layout::Flat);
    }

    #[test]
    fn the_setting_round_trips_and_falls_back_to_date() {
        for key in [
            SortKey::Date,
            SortKey::Modified,
            SortKey::Name,
            SortKey::Size,
        ] {
            for reverse in [false, true] {
                let sort = Sort { key, reverse };
                assert_eq!(Sort::from_setting(&sort.to_setting()), sort);
            }
        }
        assert_eq!(Sort::from_setting("rating"), Sort::default());
        assert_eq!(Sort::from_setting("-"), Sort::default());
        assert_eq!(Sort::from_setting(""), Sort::default());
    }

    #[test]
    fn serialises_as_camel_case() {
        let sort = Sort {
            key: SortKey::Modified,
            reverse: true,
        };
        assert_eq!(
            serde_json::to_string(&sort).unwrap(),
            r#"{"key":"modified","reverse":true}"#
        );
    }
}
