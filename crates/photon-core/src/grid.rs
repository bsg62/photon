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
    /// The newest photos by capture date, newest first and capped at
    /// `library::items::RECENT_LIMIT`. Unlike every other view this one is a window rather
    /// than a filter: it is bounded by count, not by a property of the photos.
    Recent,
    /// Photos whose file or folder name, camera, caption, keywords or date contain the engine's
    /// current search query. The query itself lives on the engine, not here: this enum is
    /// `Copy` and is mirrored in TypeScript as a union of plain strings.
    Search,
    /// Photos with a face of one Picasa contact. The contact hash is the engine's view
    /// argument, like the search query.
    Person,
    /// Photos in one of the library's albums, photon's own or Picasa's. The album id is the
    /// view argument.
    Album,
    /// Photos carrying one keyword. The keyword is the view argument.
    Tag,
    /// Photos with a byte-identical twin elsewhere in the library (`duplicates.rs`). A
    /// filter like Starred, so it keeps the folder-first order and everything built on it.
    Duplicates,
    /// One photo and its copies - the same bytes or the same picture - as its info panel
    /// lists them. The photo's id is the view argument.
    Copies,
    /// The photos the user has hidden - the one view that shows them, and the only one
    /// they are in (`library/hidden.rs`).
    Hidden,
    /// Every visible video. A filter like Starred, so a folder is placed by its oldest video
    /// and the sidebar's year groups keep agreeing with the grid.
    Videos,
}

impl GridView {
    /// Whether the view is selected by an argument held beside it on the engine. A view
    /// switch to one of these keeps the argument; a switch to any other clears it.
    pub fn takes_argument(self) -> bool {
        matches!(
            self,
            Self::Search | Self::Person | Self::Album | Self::Tag | Self::Copies
        )
    }

    /// How the view's rows are arranged on screen. Every view but Recent is ordered folder
    /// first (`GRID_ORDER`), so its rows fall into one run per folder, each under a header.
    /// Recent orders by date across folders: sectioned by folder, it started a run on every
    /// photo wherever folders interleave - 500 photos came back as 500 sections - and each
    /// run took a header and a row of its own.
    pub fn layout(self) -> Layout {
        match self {
            Self::Recent => Layout::Flat,
            _ => Layout::Folders,
        }
    }
}

/// See `GridView::layout`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// One section per run of a folder's photos, each drawn under the folder's header.
    Folders,
    /// One section holding every row, belonging to no folder and drawn with no header.
    Flat,
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
    /// A video's running time, for the tile's badge; `None` for a photo.
    pub duration_ms: Option<i64>,
    /// True when the photo is starred in Picasa's per-directory `.picasa.ini`.
    pub starred: bool,
    /// True when another live file has the same bytes or is a look-alike: membership of
    /// the Duplicates view, so the tile's mark agrees with the menu's "Show duplicates".
    pub has_copies: bool,
    /// Fingerprint of the file version. Part of thumbnail URLs, so they can be cached forever.
    #[serde(serialize_with = "serialize_hex")]
    pub thumb_key: u64,
}

/// A run of consecutive grid entries laid out together: one folder's photos under its
/// header, or, in a flat layout, every row under none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    /// The folder whose header the run is drawn under; `None` in a flat layout, whose one
    /// run spans many folders and has no header.
    pub folder_id: Option<i64>,
    pub offset: usize,
    pub count: usize,
    /// Capture time of the run's oldest photo, in seconds. The timeline's year marks read it.
    pub taken_at_min: i64,
}

/// One folder's share of the view: how many of its photos the view holds, and when the
/// oldest of them was taken. What the sidebar lists, whatever the layout - in a flat view
/// the folders are not sections, but the sidebar still names the ones the photos came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderTally {
    pub folder_id: i64,
    pub count: usize,
    /// Capture time of the folder's **oldest** photo in the view, in seconds. The sidebar
    /// groups folders by the year this falls in, resolved in the viewer's local time rather
    /// than UTC.
    ///
    /// Oldest rather than newest, to match Picasa — and because it is the sturdier of the
    /// two. A photo with no EXIF date falls back to the file's modification time
    /// (`scanner::describe`), which for an archive folder copied onto a new machine is
    /// today's date; under a newest-wins rule one such file files a folder of 1997 scans
    /// under the current year. A recent mtime cannot drag the minimum forward.
    pub taken_at_min: i64,
}

/// Ordered in-memory index the UI pages through by position.
#[derive(Debug, Default)]
pub struct GridIndex {
    entries: Vec<GridEntry>,
    sections: Vec<Section>,
    folders: Vec<FolderTally>,
    positions: HashMap<i64, usize>,
}

impl GridIndex {
    /// `entries` must already be in grid order (see `Library::grid_entries`), and `layout`
    /// is the view's (`GridView::layout`).
    pub fn build(entries: Vec<GridEntry>, layout: Layout) -> Self {
        let mut sections: Vec<Section> = Vec::new();
        let mut folders: Vec<FolderTally> = Vec::new();
        let mut tally_of: HashMap<i64, usize> = HashMap::new();
        let mut positions = HashMap::with_capacity(entries.len());
        let section_folder = |entry: &GridEntry| match layout {
            Layout::Folders => Some(entry.folder_id),
            Layout::Flat => None,
        };
        for (index, entry) in entries.iter().enumerate() {
            positions.insert(entry.id, index);
            // Real minimums, not "the run's first entry": entries are ordered by folder and
            // then by capture date, but nothing here fixes the direction, and assuming it
            // would silently file a folder under the wrong year.
            match sections.last_mut() {
                Some(section) if section.folder_id == section_folder(entry) => {
                    section.count += 1;
                    section.taken_at_min = section.taken_at_min.min(entry.taken_at);
                }
                _ => sections.push(Section {
                    folder_id: section_folder(entry),
                    offset: index,
                    count: 1,
                    taken_at_min: entry.taken_at,
                }),
            }
            // Summed over every run of the folder, not per run: where a folder's photos are
            // not contiguous, one row per run listed it once per run, each claiming a handful.
            match tally_of.get(&entry.folder_id) {
                Some(&at) => {
                    let tally = &mut folders[at];
                    tally.count += 1;
                    tally.taken_at_min = tally.taken_at_min.min(entry.taken_at);
                }
                None => {
                    tally_of.insert(entry.folder_id, folders.len());
                    folders.push(FolderTally {
                        folder_id: entry.folder_id,
                        count: 1,
                        taken_at_min: entry.taken_at,
                    });
                }
            }
        }
        Self {
            entries,
            sections,
            folders,
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

    /// The view's folders, in the order the grid first reaches each.
    pub fn folders(&self) -> &[FolderTally] {
        &self.folders
    }

    /// Where the folder's header is. `None` in a flat layout, which has no headers to land
    /// on - a folder jump switches to All first.
    pub fn offset_of_folder(&self, folder_id: i64) -> Option<usize> {
        self.sections
            .iter()
            .find(|s| s.folder_id == Some(folder_id))
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
            duration_ms: None,
            starred: false,
            has_copies: false,
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
        GridIndex::build(
            vec![
                entry(1, 10),
                entry(2, 10),
                entry(3, 20),
                entry(4, 30),
                entry(5, 30),
            ],
            Layout::Folders,
        )
    }

    #[test]
    fn builds_sections_per_folder_run() {
        let grid = sample();
        assert_eq!(grid.len(), 5);
        assert_eq!(
            grid.sections(),
            [
                Section {
                    folder_id: Some(10),
                    offset: 0,
                    count: 2,
                    taken_at_min: 1
                },
                Section {
                    folder_id: Some(20),
                    offset: 2,
                    count: 1,
                    taken_at_min: 3
                },
                Section {
                    folder_id: Some(30),
                    offset: 3,
                    count: 2,
                    taken_at_min: 4
                },
            ]
        );
        assert_eq!(grid.offset_of_folder(30), Some(3));
        assert_eq!(grid.offset_of_folder(99), None);
    }

    /// Recent interleaves folders. Laid out by folder, every photo would be a run of its own
    /// with a header of its own; flat, the rows are one headerless run.
    #[test]
    fn a_flat_layout_is_one_run_under_no_folder() {
        let rows = vec![
            entry_at(1, 10, 500),
            entry_at(2, 20, 400),
            entry_at(3, 10, 300),
            entry_at(4, 20, 200),
        ];
        assert_eq!(
            GridIndex::build(rows.clone(), Layout::Folders)
                .sections()
                .len(),
            4
        );
        let grid = GridIndex::build(rows, Layout::Flat);
        assert_eq!(
            grid.sections(),
            [Section {
                folder_id: None,
                offset: 0,
                count: 4,
                taken_at_min: 200
            }]
        );
        assert_eq!(grid.offset_of_folder(10), None, "no header to land on");
    }

    /// The sidebar lists a folder once, with every photo of it the view holds, however many
    /// runs those photos are split into - in either layout.
    #[test]
    fn a_folder_is_tallied_once_across_all_of_its_runs() {
        let rows = vec![
            entry_at(1, 10, 500),
            entry_at(2, 20, 400),
            entry_at(3, 10, 300),
            entry_at(4, 10, 200),
        ];
        let tallies = [
            FolderTally {
                folder_id: 10,
                count: 3,
                taken_at_min: 200,
            },
            FolderTally {
                folder_id: 20,
                count: 1,
                taken_at_min: 400,
            },
        ];
        assert_eq!(
            GridIndex::build(rows.clone(), Layout::Folders).folders(),
            tallies
        );
        assert_eq!(GridIndex::build(rows, Layout::Flat).folders(), tallies);
    }

    #[test]
    fn only_recent_is_laid_out_flat() {
        use GridView::*;
        for view in [
            All, Starred, Search, Person, Album, Tag, Duplicates, Copies, Hidden,
        ] {
            assert_eq!(view.layout(), Layout::Folders, "{view:?}");
        }
        assert_eq!(Recent.layout(), Layout::Flat);
    }

    #[test]
    fn rows_are_clamped() {
        let grid = sample();
        let ids = |rows: &[GridEntry]| rows.iter().map(|e| e.id).collect::<Vec<_>>();
        assert_eq!(ids(grid.rows(1, 2)), [2, 3]);
        assert_eq!(ids(grid.rows(4, 10)), [5]);
        assert!(grid.rows(10, 5).is_empty());
        assert!(
            GridIndex::build(Vec::new(), Layout::Folders)
                .rows(0, 5)
                .is_empty()
        );
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

    /// The oldest photo decides a folder's year, so the section has to take a min over its
    /// whole run. Taking the run's first entry passes only while the sort direction happens
    /// to cooperate, which nothing guarantees.
    #[test]
    fn a_sections_oldest_photo_need_not_be_its_first_entry() {
        let grid = GridIndex::build(
            vec![
                entry_at(1, 10, 900),
                entry_at(2, 10, 100),
                entry_at(3, 20, 50),
            ],
            Layout::Folders,
        );
        assert_eq!(grid.sections()[0].taken_at_min, 100);
        assert_eq!(grid.sections()[1].taken_at_min, 50);
        assert_eq!(grid.folders()[0].taken_at_min, 100);
        assert_eq!(grid.folders()[1].taken_at_min, 50);
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
    fn a_folder_takes_the_year_of_its_oldest_photo_not_its_newest() {
        // The rule that makes photon agree with Picasa, and the sturdier of the two.
        // A photo with no EXIF date falls back to the file's modification time
        // (`scanner::describe`), so a folder of 1997 scans copied onto a machine today
        // holds one entry dated today. Under a newest-wins rule that single file filed the
        // whole folder under the current year; the minimum is unmoved by it.
        let old = 883_612_800; // 1998-01-01
        let copied_today = 1_789_000_000; // a recent mtime standing in for a missing EXIF date
        let dated = |id: i64, taken_at: i64| GridEntry {
            taken_at,
            ..entry(id, 1)
        };
        let grid = GridIndex::build(vec![dated(1, old), dated(2, copied_today)], Layout::Folders);
        assert_eq!(
            grid.folders()[0].taken_at_min,
            old,
            "one file carrying today's mtime must not drag the folder out of its own era"
        );
    }

    #[test]
    fn serialises_as_camel_case() {
        let json = serde_json::to_string(&Section {
            folder_id: Some(1),
            offset: 2,
            count: 3,
            taken_at_min: 4,
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"folderId":1,"offset":2,"count":3,"takenAtMin":4}"#
        );
        let json = serde_json::to_string(&entry(7, 1)).unwrap();
        assert_eq!(
            json,
            r#"{"id":7,"folderId":1,"takenAt":7,"aspect":1.5,"kind":"image","durationMs":null,"starred":false,"hasCopies":false,"thumbKey":"000000000000002a"}"#
        );
    }
}
