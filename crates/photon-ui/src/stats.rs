//! Settings' Statistics, as rows to draw: `stats.ts`, with its tests' cases. Pure, so the
//! spelling and the searches behind the rows are held by a test.

use crate::{
    grid::labels::grouped,
    viewer_info::{camera_name, field_query, fixed},
};
use photon_core::library::LibraryStats;

/// One thing counted: a year, a camera, a lens.
#[derive(Clone, Debug, PartialEq)]
pub struct StatRow {
    pub label: String,
    pub count: i64,
    /// The row's share of the largest row in its list, nought to one, for the bar beside
    /// it. Of the largest rather than of the total: the bars compare rows, and a library
    /// spread over twenty years would otherwise draw twenty slivers.
    pub share: f64,
    /// The search that shows the row's photos, in the grammar of `search::Query`.
    pub search: String,
}

fn rows(items: Vec<(String, i64, String)>) -> Vec<StatRow> {
    let most = items
        .iter()
        .map(|(_, count, _)| *count)
        .max()
        .unwrap_or(1)
        .max(1);
    (items.into_iter())
        .map(|(label, count, search)| StatRow {
            label,
            count,
            share: count as f64 / most as f64,
            search,
        })
        .collect()
}

/// "1 photo" / "1,234 photos".
fn counted(n: i64, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{} {many}", grouped(usize::try_from(n).unwrap_or(0)))
    }
}

/// A library's size on disk: whole megabytes below a gigabyte, gigabytes with one decimal,
/// terabytes with two, in binary units - `librarySize`, its ties rounded as `toFixed`
/// rounds them.
pub fn library_size(bytes: i64) -> String {
    let mb = bytes as f64 / 1_048_576.0;
    if mb < 1024.0 {
        return format!("{} MB", mb.round());
    }
    let gb = mb / 1024.0;
    if gb < 1024.0 {
        format!("{} GB", fixed(gb, 1))
    } else {
        format!("{} TB", fixed(gb / 1024.0, 2))
    }
}

/// The year of a capture time: a camera's wall clock in naive seconds, read in UTC.
fn year(seconds: i64) -> Option<i16> {
    let at = jiff::Timestamp::from_second(seconds).ok()?;
    Some(at.to_zoned(jiff::tz::TimeZone::UTC).year())
}

/// The line under the heading: "12,034 photos and 310 videos · 48.2 GB · 2004 to 2026".
/// Videos are named only when there are some, and the years only when there is a photo
/// to date; a library of one year says the year once.
pub fn summary(stats: &LibraryStats) -> String {
    let mut first = counted(stats.photos, "photo", "photos");
    if stats.videos > 0 {
        first.push_str(&format!(
            " and {}",
            counted(stats.videos, "video", "videos")
        ));
    }
    let mut parts = vec![first, library_size(stats.bytes)];
    if let (Some(from), Some(to)) = (stats.oldest.and_then(year), stats.newest.and_then(year)) {
        parts.push(if from == to {
            from.to_string()
        } else {
            format!("{from} to {to}")
        });
    }
    parts.join(" · ")
}

/// The years, newest first as the library sends them. A year's row is a search of
/// exactly that year.
pub fn year_rows(stats: &LibraryStats) -> Vec<StatRow> {
    rows(
        (stats.years.iter())
            .map(|y| {
                (
                    y.year.to_string(),
                    y.count,
                    format!("from:{0} to:{0}", y.year),
                )
            })
            .collect(),
    )
}

/// The cameras, named and searched for as the info panel names and searches them. The
/// library counts by make and model as the files spell them, and `camera_name` folds some
/// of those pairs into one name - "Canon EOS 5D" with the make "Canon" and with none are
/// one camera to anyone reading the list - so their counts are added and the list sorted
/// again, a name appearing once. The count is of that exact camera; its search, of the
/// camera field word by word, can find more ("EOS 5D" finds an "EOS 5D Mark IV" too).
pub fn camera_rows(stats: &LibraryStats) -> Vec<StatRow> {
    let mut by_name: Vec<(String, i64)> = Vec::new();
    for camera in &stats.cameras {
        let Some(name) = camera_name(camera.make.as_deref(), camera.model.as_deref()) else {
            continue;
        };
        match by_name.iter_mut().find(|(seen, _)| *seen == name) {
            Some((_, count)) => *count += camera.count,
            None => by_name.push((name, camera.count)),
        }
    }
    // Stable: equal counts keep the library's order, which is by name.
    by_name.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    rows(
        (by_name.into_iter())
            .map(|(label, count)| {
                let search = field_query("camera", &label);
                (label, count, search)
            })
            .collect(),
    )
}

pub fn lens_rows(stats: &LibraryStats) -> Vec<StatRow> {
    rows(
        (stats.lenses.iter())
            .map(|lens| {
                (
                    lens.lens.clone(),
                    lens.count,
                    field_query("lens", &lens.lens),
                )
            })
            .collect(),
    )
}

/// The section's words.
pub const COUNTING: &str = "Counting…";
pub const NOT_COUNTED: &str = "The library could not be counted.";
pub const SUMMARY_TAIL: &str = "Hidden photos are not counted. Click a row to search for it.";
pub const YEARS: &str = "Years";
pub const CAMERAS: &str = "Cameras";
pub const LENSES: &str = "Lenses";

/// "3 without camera data.", said after the lists when some photos have no camera and
/// others have one.
pub fn without_camera(count: i64) -> String {
    format!(
        "{} without camera data.",
        grouped(usize::try_from(count).unwrap_or(0))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use photon_core::library::{CameraCount, LensCount, YearCount};

    fn empty() -> LibraryStats {
        LibraryStats {
            photos: 0,
            videos: 0,
            bytes: 0,
            oldest: None,
            newest: None,
            years: Vec::new(),
            cameras: Vec::new(),
            no_camera: 0,
            lenses: Vec::new(),
        }
    }

    fn utc(y: i16, m: i8, d: i8) -> i64 {
        jiff::civil::date(y, m, d)
            .to_zoned(jiff::tz::TimeZone::UTC)
            .unwrap()
            .timestamp()
            .as_second()
    }

    const GB: i64 = 1_073_741_824;
    const MB: i64 = 1_048_576;

    #[test]
    fn the_summary_names_photos_videos_the_size_and_the_years() {
        let stats = LibraryStats {
            photos: 12034,
            videos: 310,
            bytes: 5 * GB,
            oldest: Some(utc(2004, 3, 1)),
            newest: Some(utc(2026, 7, 14)),
            ..empty()
        };
        assert_eq!(
            summary(&stats),
            "12,034 photos and 310 videos · 5.0 GB · 2004 to 2026"
        );
        let one = LibraryStats {
            photos: 1,
            bytes: 2 * MB,
            oldest: Some(utc(2020, 1, 1)),
            newest: Some(utc(2020, 12, 31)),
            ..empty()
        };
        assert_eq!(summary(&one), "1 photo · 2 MB · 2020");
        assert_eq!(summary(&empty()), "0 photos · 0 MB");
    }

    // The last second of 2019 and the first of 2020, which a zone either side of UTC
    // would put in one year.
    #[test]
    fn the_year_is_read_off_the_camera_clock() {
        let stats = LibraryStats {
            photos: 2,
            oldest: Some(utc(2020, 1, 1) - 1),
            newest: Some(utc(2020, 1, 1)),
            ..empty()
        };
        assert!(summary(&stats).contains("2019 to 2020"));
    }

    #[test]
    fn the_size_steps_from_megabytes_to_gigabytes_to_terabytes() {
        assert_eq!(library_size(GB - MB), "1023 MB");
        assert_eq!(library_size(GB), "1.0 GB");
        assert_eq!(library_size(48 * GB + GB / 4), "48.3 GB");
        assert_eq!(library_size(1024 * GB), "1.00 TB");
        assert_eq!(library_size(2560 * GB), "2.50 TB");
    }

    #[test]
    fn a_year_is_a_search_of_that_year_and_the_bars_are_of_the_largest() {
        let rows = year_rows(&LibraryStats {
            years: vec![
                YearCount {
                    year: 2024,
                    count: 50,
                },
                YearCount {
                    year: 2019,
                    count: 200,
                },
                YearCount {
                    year: 2001,
                    count: 1,
                },
            ],
            ..empty()
        });
        let found: Vec<_> = (rows.iter())
            .map(|r| (r.label.as_str(), r.count, r.search.as_str(), r.share))
            .collect();
        assert_eq!(
            found,
            [
                ("2024", 50, "from:2024 to:2024", 0.25),
                ("2019", 200, "from:2019 to:2019", 1.0),
                ("2001", 1, "from:2001 to:2001", 0.005),
            ]
        );
        assert!(year_rows(&empty()).is_empty());
    }

    #[test]
    fn a_camera_is_named_as_the_info_panel_names_it_and_alike_names_are_added() {
        let camera = |make: Option<&str>, model: &str, count| CameraCount {
            make: make.map(str::to_owned),
            model: Some(model.to_owned()),
            count,
        };
        let rows = camera_rows(&LibraryStats {
            cameras: vec![
                camera(Some("Canon"), "Canon EOS 5D", 30),
                camera(Some("Apple"), "iPhone 15", 15),
                camera(None, "Scanner \"Pro\"", 3),
            ],
            ..empty()
        });
        let found: Vec<_> = (rows.iter())
            .map(|r| (r.label.as_str(), r.search.as_str(), r.share))
            .collect();
        assert_eq!(
            found,
            [
                ("Canon EOS 5D", "camera:\"Canon EOS 5D\"", 1.0),
                ("Apple iPhone 15", "camera:\"Apple iPhone 15\"", 0.5),
                ("Scanner \"Pro\"", "camera:\"Scanner  Pro\"", 0.1),
            ]
        );
        let merged = camera_rows(&LibraryStats {
            cameras: vec![
                camera(Some("Apple"), "iPhone 15", 50),
                camera(Some("Canon"), "Canon EOS 5D", 40),
                camera(None, "Canon EOS 5D", 20),
            ],
            ..empty()
        });
        let found: Vec<_> = (merged.iter())
            .map(|r| (r.label.as_str(), r.count))
            .collect();
        assert_eq!(found, [("Canon EOS 5D", 60), ("Apple iPhone 15", 50)]);
    }

    #[test]
    fn a_lens_is_a_search_of_its_field() {
        let rows = lens_rows(&LibraryStats {
            lenses: vec![LensCount {
                lens: "EF50mm f/1.8 STM".to_owned(),
                count: 4,
            }],
            ..empty()
        });
        assert_eq!(rows[0].search, "lens:\"EF50mm f/1.8 STM\"");
        assert_eq!(rows[0].share, 1.0);
    }
}
