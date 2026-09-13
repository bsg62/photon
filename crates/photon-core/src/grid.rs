use crate::media::MediaKind;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashMap;

/// Which set of photos the grid shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GridView {
    #[default]
    All,
    Starred,
    /// Photos whose file or folder name contains the engine's current search query.
    /// The query itself lives on the engine, not here: this enum is `Copy` and is
    /// mirrored in TypeScript as a union of plain strings.
    Search,
}

/// A u64 as 16 lowercase hex characters: exact in JavaScript, unlike a JSON number.
pub fn hex_key(value: u64) -> String {
    format!("{value:016x}")
}

fn serialize_hex<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&hex_key(*value))
}

/// One cell of the library grid. Small and `Copy`: 100k of them stay in memory.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridEntry {
    pub id: i64,
    pub folder_id: i64,
    pub taken_at: i64,
    /// Displayed width / height (orientation applied); 1.0 when unknown.
    pub aspect: f32,
    pub kind: MediaKind,
    /// True when the photo carries at least one star in its XMP rating.
    pub starred: bool,
    /// Fingerprint of the file version. Part of thumbnail URLs, so they can be cached forever.
    #[serde(serialize_with = "serialize_hex")]
    pub thumb_key: u64,
}

/// A run of consecutive grid entries from one folder, shown under one header.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    pub folder_id: i64,
    pub offset: usize,
    pub count: usize,
    /// Capture time of the folder's newest photo, in seconds. The sidebar groups folders by
    /// the year this falls in, resolved in the viewer's local time rather than UTC.
    pub taken_at_max: i64,
}

/// Ordered in-memory index the UI pages through by position.
#[derive(Debug, Default)]
pub struct GridIndex {
    entries: Vec<GridEntry>,
    sections: Vec<Section>,
    positions: HashMap<i64, usize>,
}

impl GridIndex {
    /// `entries` must already be in grid order (see `Library::grid_entries`).
    pub fn build(entries: Vec<GridEntry>) -> Self {
        let mut sections: Vec<Section> = Vec::new();
        let mut positions = HashMap::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            positions.insert(entry.id, index);
            match sections.last_mut() {
                Some(section) if section.folder_id == entry.folder_id => {
                    section.count += 1;
                    // A real max, not "the run's last entry": entries are ordered by folder
                    // and then by capture date, but nothing here fixes the direction, and
                    // assuming it would silently file a folder under the wrong year.
                    section.taken_at_max = section.taken_at_max.max(entry.taken_at);
                }
                _ => sections.push(Section {
                    folder_id: entry.folder_id,
                    offset: index,
                    count: 1,
                    taken_at_max: entry.taken_at,
                }),
            }
        }
        Self {
            entries,
            sections,
            positions,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn rows(&self, offset: usize, count: usize) -> &[GridEntry] {
        let start = offset.min(self.entries.len());
        let end = start.saturating_add(count).min(self.entries.len());
        &self.entries[start..end]
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    pub fn offset_of_folder(&self, folder_id: i64) -> Option<usize> {
        self.sections
            .iter()
            .find(|s| s.folder_id == folder_id)
            .map(|s| s.offset)
    }

    pub fn position_of(&self, id: i64) -> Option<usize> {
        self.positions.get(&id).copied()
    }

    /// Items around `id`, nearest first (+1, -1, +2, -2, …), for viewer preloading.
    pub fn neighbours(&self, id: i64, radius: usize) -> Vec<i64> {
        let Some(pos) = self.position_of(id) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(radius * 2);
        for distance in 1..=radius {
            if let Some(next) = self.entries.get(pos + distance) {
                out.push(next.id);
            }
            if let Some(prev) = pos.checked_sub(distance) {
                out.push(self.entries[prev].id);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: i64, folder_id: i64) -> GridEntry {
        GridEntry {
            id,
            folder_id,
            taken_at: id,
            aspect: 1.5,
            kind: MediaKind::Image,
            starred: false,
            thumb_key: 42,
        }
    }

    /// `entry` ties `taken_at` to `id`, which makes every run coincidentally ascending.
    /// This one sets the capture time independently.
    fn entry_at(id: i64, folder_id: i64, taken_at: i64) -> GridEntry {
        GridEntry {
            taken_at,
            ..entry(id, folder_id)
        }
    }

    fn sample() -> GridIndex {
        GridIndex::build(vec![
            entry(1, 10),
            entry(2, 10),
            entry(3, 20),
            entry(4, 30),
            entry(5, 30),
        ])
    }

    #[test]
    fn builds_sections_per_folder_run() {
        let grid = sample();
        assert_eq!(grid.len(), 5);
        assert_eq!(
            grid.sections(),
            [
                Section {
                    folder_id: 10,
                    offset: 0,
                    count: 2,
                    taken_at_max: 2
                },
                Section {
                    folder_id: 20,
                    offset: 2,
                    count: 1,
                    taken_at_max: 3
                },
                Section {
                    folder_id: 30,
                    offset: 3,
                    count: 2,
                    taken_at_max: 5
                },
            ]
        );
        assert_eq!(grid.offset_of_folder(30), Some(3));
        assert_eq!(grid.offset_of_folder(99), None);
    }

    #[test]
    fn rows_are_clamped() {
        let grid = sample();
        let ids = |rows: &[GridEntry]| rows.iter().map(|e| e.id).collect::<Vec<_>>();
        assert_eq!(ids(grid.rows(1, 2)), [2, 3]);
        assert_eq!(ids(grid.rows(4, 10)), [5]);
        assert!(grid.rows(10, 5).is_empty());
        assert!(GridIndex::build(Vec::new()).rows(0, 5).is_empty());
    }

    #[test]
    fn positions_and_neighbours() {
        let grid = sample();
        assert_eq!(grid.position_of(4), Some(3));
        assert_eq!(grid.position_of(99), None);
        assert_eq!(grid.neighbours(3, 2), [4, 2, 5, 1]);
        assert_eq!(grid.neighbours(1, 2), [2, 3]);
        assert!(grid.neighbours(99, 2).is_empty());
    }

    /// The newest photo decides a folder's year, so the section has to take a max over its
    /// whole run. Taking the run's last entry passes only while the sort direction happens
    /// to cooperate, which nothing guarantees.
    #[test]
    fn a_sections_newest_photo_need_not_be_its_last_entry() {
        let grid = GridIndex::build(vec![
            entry_at(1, 10, 900),
            entry_at(2, 10, 100),
            entry_at(3, 20, 50),
        ]);
        assert_eq!(grid.sections()[0].taken_at_max, 900);
        assert_eq!(grid.sections()[1].taken_at_max, 50);
    }

    #[test]
    fn entries_carry_whether_they_are_starred() {
        let json = serde_json::to_string(&GridEntry {
            starred: true,
            ..entry(7, 1)
        })
        .unwrap();
        assert!(json.contains(r#""starred":true"#), "got {json}");
    }

    #[test]
    fn serialises_as_camel_case() {
        let json = serde_json::to_string(&Section {
            folder_id: 1,
            offset: 2,
            count: 3,
            taken_at_max: 4,
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"folderId":1,"offset":2,"count":3,"takenAtMax":4}"#
        );
        let json = serde_json::to_string(&entry(7, 1)).unwrap();
        assert_eq!(
            json,
            r#"{"id":7,"folderId":1,"takenAt":7,"aspect":1.5,"kind":"image","starred":false,"thumbKey":"000000000000002a"}"#
        );
    }
}
