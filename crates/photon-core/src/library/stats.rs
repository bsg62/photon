//! What the library holds, counted: how many photos and videos, how much disk, and how they
//! spread over the years, the cameras and the lenses.
//!
//! Asked for by the Settings dialog when its Statistics section is opened, and by nothing
//! else: it reads every visible photo, so it is not something to hang on `library_changed`.
//! Hidden photos are left out, as they are from every other count the user sees.

use super::Library;
use crate::Result;
use crate::media::MediaKind;
use crate::metadata::civil_from_unix;
use serde::Serialize;

/// How many cameras, and how many lenses, the statistics name. The list is for "what did I
/// shoot most with", not an inventory: past the tenth a library's cameras are a tail of
/// phones borrowed for one afternoon.
pub const STATS_TOP: usize = 10;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    pub photos: i64,
    pub videos: i64,
    /// The files' sizes added up, photos and videos alike.
    pub bytes: i64,
    /// The earliest and latest capture time, in the naive seconds `taken_at` holds; `None`
    /// for an empty library.
    pub oldest: Option<i64>,
    pub newest: Option<i64>,
    /// Newest year first. A year with nothing in it is absent.
    pub years: Vec<YearCount>,
    /// The [`STATS_TOP`] cameras with the most photos and videos, most first.
    pub cameras: Vec<CameraCount>,
    /// How many photos and videos record no camera at all: scans, screenshots, most of what
    /// a chat app has re-saved.
    pub no_camera: i64,
    /// The [`STATS_TOP`] lenses with the most photos, most first.
    pub lenses: Vec<LensCount>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct YearCount {
    pub year: i64,
    pub count: i64,
}

/// A camera as the file names it. Make and model stay apart so the UI can spell the pair
/// the way the info panel does (`cameraName`), and search for it the same way.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraCount {
    pub make: Option<String>,
    pub model: Option<String>,
    pub count: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LensCount {
    pub lens: String,
    pub count: i64,
}

/// Every visible photo's contribution to the statistics, shared with its plan test. The `+`
/// keeps it a scan in table order rather than a walk of a partial index on that predicate
/// (`library/mod.rs`).
const STATS_SQL: &str = "SELECT taken_at, kind, size, make, model, lens FROM items
     WHERE +missing_since IS NULL AND hidden = 0";

/// What separates make from model in the camera map's key. Neither can hold one: a NUL ends
/// an EXIF ASCII field, and `metadata::ascii_text` trims it.
const KEY_SEPARATOR: char = '\0';

impl Library {
    /// Counts the library. One scan, counted in Rust: the totals, the years and both lists
    /// then describe the same snapshot, where four queries could each see a different one
    /// while a scan is writing, and a year needs `civil_from_unix` either way - the naive
    /// seconds `taken_at` holds are the reading the grid and search make of a date.
    pub fn stats(&self) -> Result<LibraryStats> {
        let conn = self.reader()?;
        let mut stmt = conn.prepare(STATS_SQL)?;
        let mut rows = stmt.query([])?;
        let mut stats = LibraryStats::default();
        let mut years = foldhash::HashMap::<i64, i64>::default();
        let mut cameras = foldhash::HashMap::<String, i64>::default();
        let mut lenses = foldhash::HashMap::<String, i64>::default();
        let mut key = String::new();
        while let Some(r) = rows.next()? {
            let taken: i64 = r.get(0)?;
            match MediaKind::from_db(r.get(1)?) {
                Some(MediaKind::Video) => stats.videos += 1,
                _ => stats.photos += 1,
            }
            stats.bytes += r.get::<_, i64>(2)?;
            stats.oldest = Some(stats.oldest.map_or(taken, |t| t.min(taken)));
            stats.newest = Some(stats.newest.map_or(taken, |t| t.max(taken)));
            *years.entry(civil_from_unix(taken).0).or_default() += 1;

            // Borrowed from the row, so a name is copied once per camera, not once per photo.
            let make = r.get_ref(3)?.as_str_or_null().unwrap_or(None);
            let model = r.get_ref(4)?.as_str_or_null().unwrap_or(None);
            if make.is_none() && model.is_none() {
                stats.no_camera += 1;
            } else {
                key.clear();
                key.push_str(make.unwrap_or(""));
                key.push(KEY_SEPARATOR);
                key.push_str(model.unwrap_or(""));
                count(&mut cameras, &key);
            }
            if let Some(lens) = r.get_ref(5)?.as_str_or_null().unwrap_or(None) {
                count(&mut lenses, lens);
            }
        }

        stats.years = years
            .into_iter()
            .map(|(year, count)| YearCount { year, count })
            .collect();
        stats.years.sort_by_key(|y| std::cmp::Reverse(y.year));
        stats.cameras = top(cameras)
            .into_iter()
            .map(|(key, count)| {
                let (make, model) = key.split_once(KEY_SEPARATOR).unwrap_or((&key, ""));
                let part = |text: &str| (!text.is_empty()).then(|| text.to_string());
                CameraCount {
                    make: part(make),
                    model: part(model),
                    count,
                }
            })
            .collect();
        stats.lenses = top(lenses)
            .into_iter()
            .map(|(lens, count)| LensCount { lens, count })
            .collect();
        Ok(stats)
    }
}

fn count(map: &mut foldhash::HashMap<String, i64>, name: &str) {
    match map.get_mut(name) {
        Some(n) => *n += 1,
        None => {
            map.insert(name.to_owned(), 1);
        }
    }
}

/// The [`STATS_TOP`] names with the highest counts, highest first; equal counts by name, so
/// the list does not reshuffle between two openings of the dialog.
fn top(map: foldhash::HashMap<String, i64>) -> Vec<(String, i64)> {
    let mut all: Vec<(String, i64)> = map.into_iter().collect();
    all.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    all.truncate(STATS_TOP);
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::NewItem;
    use crate::metadata::{CameraMeta, naive_to_unix};
    use crate::testutil::{new_item, seed_folder, temp_library};
    use std::path::Path;

    fn shot(folder: i64, name: &str, year: i64, camera: (&str, &str), lens: &str) -> NewItem {
        let text = |t: &str| (!t.is_empty()).then(|| t.to_string());
        NewItem {
            camera: CameraMeta {
                make: text(camera.0),
                model: text(camera.1),
                lens: text(lens),
                ..CameraMeta::default()
            },
            ..new_item(folder, name, naive_to_unix(year, 6, 15, 12, 0, 0))
        }
    }

    #[test]
    fn an_empty_library_counts_nothing() {
        let (_dir, lib) = temp_library();
        assert_eq!(lib.stats().unwrap(), LibraryStats::default());
    }

    #[test]
    fn the_library_is_counted_by_kind_year_camera_and_lens() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let mut clip = shot(folder, "/p/clip.mp4", 2024, ("Apple", "iPhone 15"), "");
        clip.kind = MediaKind::Video;
        let ids = lib
            .insert_items(&[
                shot(folder, "/p/a.jpg", 2019, ("Canon", "EOS 5D"), "EF50mm"),
                shot(folder, "/p/b.jpg", 2019, ("Canon", "EOS 5D"), "EF85mm"),
                shot(folder, "/p/c.jpg", 2024, ("Canon", "EOS 5D"), "EF50mm"),
                shot(folder, "/p/d.jpg", 2024, ("Apple", "iPhone 15"), ""),
                // The same model under another make is another camera.
                shot(folder, "/p/e.jpg", 2024, ("", "EOS 5D"), ""),
                shot(folder, "/p/scan.jpg", 2001, ("", ""), ""),
                clip,
                shot(folder, "/p/hidden.jpg", 1999, ("Leica", "M6"), "Summicron"),
                shot(folder, "/p/gone.jpg", 1998, ("Leica", "M6"), "Summicron"),
            ])
            .unwrap();
        lib.set_hidden(&[ids[7]], true).unwrap();
        lib.mark_missing(&[ids[8]], 1).unwrap();

        let stats = lib.stats().unwrap();
        assert_eq!((stats.photos, stats.videos), (6, 1));
        // `new_item` gives every file 100 bytes.
        assert_eq!(stats.bytes, 700);
        assert_eq!(stats.oldest, Some(naive_to_unix(2001, 6, 15, 12, 0, 0)));
        assert_eq!(stats.newest, Some(naive_to_unix(2024, 6, 15, 12, 0, 0)));
        let years: Vec<(i64, i64)> = stats.years.iter().map(|y| (y.year, y.count)).collect();
        assert_eq!(years, [(2024, 4), (2019, 2), (2001, 1)]);
        let cameras: Vec<(Option<&str>, Option<&str>, i64)> = stats
            .cameras
            .iter()
            .map(|c| (c.make.as_deref(), c.model.as_deref(), c.count))
            .collect();
        assert_eq!(
            cameras,
            [
                (Some("Canon"), Some("EOS 5D"), 3),
                (Some("Apple"), Some("iPhone 15"), 2),
                (None, Some("EOS 5D"), 1),
            ]
        );
        assert_eq!(stats.no_camera, 1);
        let lenses: Vec<(&str, i64)> = stats
            .lenses
            .iter()
            .map(|l| (l.lens.as_str(), l.count))
            .collect();
        assert_eq!(lenses, [("EF50mm", 2), ("EF85mm", 1)]);
    }

    #[test]
    fn only_the_top_cameras_are_named_and_ties_go_by_name() {
        let (_dir, lib) = temp_library();
        let (_w, folder) = seed_folder(&lib, Path::new("/p"));
        let mut items = Vec::new();
        // Twelve cameras with one photo each, and one with two.
        for n in 0..12 {
            let model = format!("M{n:02}");
            items.push(shot(
                folder,
                &format!("/p/{n}.jpg"),
                2020,
                ("Acme", &model),
                "",
            ));
        }
        items.push(shot(folder, "/p/extra.jpg", 2020, ("Acme", "M07"), ""));
        lib.insert_items(&items).unwrap();
        let models: Vec<String> = lib
            .stats()
            .unwrap()
            .cameras
            .into_iter()
            .map(|c| c.model.unwrap())
            .collect();
        assert_eq!(
            models,
            [
                "M07", "M00", "M01", "M02", "M03", "M04", "M05", "M06", "M08", "M09"
            ]
        );
    }

    /// Pins the `+` in `STATS_SQL`: without it the planner walks a partial index on
    /// `missing_since IS NULL` in random table order (`library/mod.rs`).
    #[test]
    fn the_statistics_are_read_by_a_scan_in_table_order() {
        let (_dir, lib) = temp_library();
        assert_eq!(lib.query_plan(STATS_SQL, &[]), ["SCAN items"]);
    }
}
