use crate::media::MediaKind;
use serde::{Deserialize, Serialize, Serializer};

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

/// See `GridView::layout` and `sort::Sort::layout`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// One section per run of a folder's photos, each drawn under the folder's header.
    #[default]
    Folders,
    /// One section holding every row, belonging to no folder and drawn with no header.
    Flat,
    /// One section per day, month or year the rows pass through, each under a header naming
    /// it. The rows must already be in date order (`sort::Sort::arrange`), or a period comes
    /// back as a second section the way a folder's interleaved photos do.
    Periods(PeriodUnit),
}

/// How long a stretch one header of a date grouping covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeriodUnit {
    Day,
    Month,
    Year,
}

impl PeriodUnit {
    /// The period a capture time falls in. Read as the camera's wall-clock time
    /// (`civil_from_unix`), the reading search's `2024-06` and Statistics use, so a month's
    /// header and a search for that month hold the same photos.
    pub fn of(self, taken_at: i64) -> Period {
        let (year, month, day) = crate::metadata::civil_from_unix(taken_at);
        match self {
            Self::Day => Period {
                year,
                month: Some(month),
                day: Some(day),
            },
            Self::Month => Period {
                year,
                month: Some(month),
                day: None,
            },
            Self::Year => Period {
                year,
                month: None,
                day: None,
            },
        }
    }
}

/// A day, a month or a year: a month has no `day`, a year neither. Sent as its numbers
/// rather than as a timestamp, so the UI names the day Rust put the photos in instead of
/// reading an instant again in the viewer's own zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Period {
    pub year: i64,
    pub month: Option<u32>,
    pub day: Option<u32>,
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
    /// The file's size in bytes and modification time, for `sort::Sort` and the sidebar's
    /// folder totals. Not sent to the UI: it pages through rows in the order they are
    /// already in, and nothing on a tile shows either.
    #[serde(skip)]
    pub size: i64,
    #[serde(skip)]
    pub mtime_ms: i64,
}

/// A run of consecutive grid entries laid out together: one folder's photos under its
/// header, one period's under its own, or, in a flat layout, every row under none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    /// The folder whose header the run is drawn under; `None` in every other layout, whose
    /// runs span many folders.
    pub folder_id: Option<i64>,
    pub offset: usize,
    pub count: usize,
    /// Capture time of the run's oldest photo, in seconds. The timeline's year marks read it.
    pub taken_at_min: i64,
    /// The day, month or year the run is drawn under; `None` unless the layout is by period.
    pub period: Option<Period>,
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
    /// The folder's photos in the view, in bytes. What the sidebar orders by under a size
    /// sort: the folder that holds the most, as a total, since that is the question a size
    /// sort of folders answers.
    pub bytes: i64,
    /// The newest modification time among the folder's photos in the view, in
    /// milliseconds. What the sidebar orders by under a modified sort: the folder most
    /// recently touched.
    pub modified_ms: i64,
}

/// Ordered in-memory index the UI pages through by position.
#[derive(Debug, Default)]
pub struct GridIndex {
    entries: Vec<GridEntry>,
    sections: Vec<Section>,
    folders: Vec<FolderTally>,
    positions: foldhash::HashMap<i64, usize>,
    layout: Layout,
}

impl GridIndex {
    /// `entries` must already be in grid order (see `Library::grid_entries`), and `layout`
    /// is the view's (`GridView::layout`).
    pub fn build(entries: Vec<GridEntry>, layout: Layout) -> Self {
        let mut sections: Vec<Section> = Vec::new();
        let mut folders: Vec<FolderTally> = Vec::new();
        // foldhash, not std's SipHash, for maps with a key per photo: `startup_grid_100k`
        // 58.9 -> 57.3 ms. The ids are the library's own, so there is no hostile input for
        // SipHash's flooding resistance to guard against.
        let mut tally_of = foldhash::HashMap::<i64, usize>::default();
        let mut positions =
            foldhash::HashMap::with_capacity_and_hasher(entries.len(), Default::default());
        // What a run is of. A new section starts wherever this differs from the last one's.
        let section_key = |entry: &GridEntry| match layout {
            Layout::Folders => (Some(entry.folder_id), None),
            Layout::Flat => (None, None),
            Layout::Periods(unit) => (None, Some(unit.of(entry.taken_at))),
        };
        for (index, entry) in entries.iter().enumerate() {
            positions.insert(entry.id, index);
            let (folder_id, period) = section_key(entry);
            // Real minimums, not "the run's first entry": entries are ordered by folder and
            // then by capture date, but nothing here fixes the direction, and assuming it
            // would silently file a folder under the wrong year.
            match sections.last_mut() {
                Some(section) if section.folder_id == folder_id && section.period == period => {
                    section.count += 1;
                    section.taken_at_min = section.taken_at_min.min(entry.taken_at);
                }
                _ => sections.push(Section {
                    folder_id,
                    period,
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
                    tally.bytes += entry.size;
                    tally.modified_ms = tally.modified_ms.max(entry.mtime_ms);
                }
                None => {
                    tally_of.insert(entry.folder_id, folders.len());
                    folders.push(FolderTally {
                        folder_id: entry.folder_id,
                        count: 1,
                        taken_at_min: entry.taken_at,
                        bytes: entry.size,
                        modified_ms: entry.mtime_ms,
                    });
                }
            }
        }
        Self {
            entries,
            sections,
            folders,
            positions,
            layout,
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

    /// Where a jump to the folder lands: its header, or in a layout that gives folders no
    /// header (flat, or by period), the first of its photos the grid reaches. The flat answer
    /// is what makes a sidebar click do something under a sort other than date, where All
    /// itself is flat; Recent, the other flat view, is never asked, since a folder jump
    /// switches to All.
    pub fn offset_of_folder(&self, folder_id: i64) -> Option<usize> {
        match self.layout {
            Layout::Folders => self
                .sections
                .iter()
                .find(|s| s.folder_id == Some(folder_id))
                .map(|s| s.offset),
            Layout::Flat | Layout::Periods(_) => {
                self.entries.iter().position(|e| e.folder_id == folder_id)
            }
        }
    }

    /// The photos of the folder the photo at `offset` belongs to, in grid order; `None` past
    /// the end. What Ctrl+A selects in All under a flat sort, where the folder's photos are
    /// scattered through the run rather than a section a range could name.
    pub fn folder_ids_at(&self, offset: usize) -> Option<Vec<i64>> {
        let folder_id = self.entries.get(offset)?.folder_id;
        Some(
            self.entries
                .iter()
                .filter(|e| e.folder_id == folder_id)
                .map(|e| e.id)
                .collect(),
        )
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
            size: 0,
            mtime_ms: 0,
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
                    taken_at_min: 1,
                    period: None
                },
                Section {
                    folder_id: Some(20),
                    offset: 2,
                    count: 1,
                    taken_at_min: 3,
                    period: None
                },
                Section {
                    folder_id: Some(30),
                    offset: 3,
                    count: 2,
                    taken_at_min: 4,
                    period: None
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
                taken_at_min: 200,
                period: None
            }]
        );
        // No header to land on: a jump lands on the first of the folder's photos instead.
        assert_eq!(grid.offset_of_folder(10), Some(0));
        assert_eq!(grid.offset_of_folder(20), Some(1));
        assert_eq!(grid.offset_of_folder(99), None);
    }

    /// Under a sort other than date a folder's first photo need not be its first run's
    /// first photo, nor where its header would have been: the flat answer is a position in
    /// the rows, not in the sections.
    #[test]
    fn a_flat_jump_lands_on_the_folders_first_photo_wherever_it_is() {
        let rows = vec![
            entry_at(1, 20, 500),
            entry_at(2, 20, 400),
            entry_at(3, 10, 300),
            entry_at(4, 20, 200),
        ];
        let grid = GridIndex::build(rows.clone(), Layout::Flat);
        assert_eq!(grid.offset_of_folder(10), Some(2));
        let folders = GridIndex::build(rows, Layout::Folders);
        assert_eq!(folders.offset_of_folder(20), Some(0));
        assert_eq!(folders.offset_of_folder(10), Some(2));
    }

    #[test]
    fn a_folders_ids_are_found_wherever_the_flat_run_scattered_them() {
        let grid = GridIndex::build(
            vec![
                entry_at(1, 20, 500),
                entry_at(2, 10, 400),
                entry_at(3, 20, 300),
                entry_at(4, 30, 200),
                entry_at(5, 20, 100),
            ],
            Layout::Flat,
        );
        assert_eq!(grid.folder_ids_at(2), Some(vec![1, 3, 5]));
        assert_eq!(grid.folder_ids_at(1), Some(vec![2]));
        assert_eq!(grid.folder_ids_at(5), None);
    }

    /// The sidebar's size and modified orders read these, summed and maxed over every run.
    #[test]
    fn a_folder_totals_its_bytes_and_keeps_its_newest_modification() {
        let sized = |id: i64, folder_id: i64, size: i64, mtime_ms: i64| GridEntry {
            size,
            mtime_ms,
            ..entry(id, folder_id)
        };
        let grid = GridIndex::build(
            vec![
                sized(1, 10, 100, 7_000),
                sized(2, 20, 5, 1_000),
                sized(3, 10, 40, 9_000),
                sized(4, 10, 1, 8_000),
            ],
            Layout::Flat,
        );
        let totals: Vec<_> = grid
            .folders()
            .iter()
            .map(|t| (t.folder_id, t.bytes, t.modified_ms))
            .collect();
        assert_eq!(totals, [(10, 141, 9_000), (20, 5, 1_000)]);
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
                bytes: 0,
                modified_ms: 0,
            },
            FolderTally {
                folder_id: 20,
                count: 1,
                taken_at_min: 400,
                bytes: 0,
                modified_ms: 0,
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
            period: None,
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"folderId":1,"offset":2,"count":3,"takenAtMin":4,"period":null}"#
        );
        let json = serde_json::to_string(&entry(7, 1)).unwrap();
        assert_eq!(
            json,
            r#"{"id":7,"folderId":1,"takenAt":7,"aspect":1.5,"kind":"image","durationMs":null,"starred":false,"hasCopies":false,"thumbKey":"000000000000002a"}"#
        );
    }

    /// Seconds for a wall-clock time, the way `taken_at` holds one.
    fn at(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
        crate::metadata::naive_to_unix(year, month, day, hour, minute, second)
    }

    /// Newest first, across two folders: a second either side of new year's midnight, then
    /// earlier on the 31st, then a month before.
    fn timeline() -> Vec<GridEntry> {
        vec![
            entry_at(1, 10, at(2026, 1, 1, 0, 0, 0)),
            entry_at(2, 20, at(2025, 12, 31, 23, 59, 59)),
            entry_at(3, 10, at(2025, 12, 31, 8, 0, 0)),
            entry_at(4, 20, at(2025, 11, 30, 12, 0, 0)),
        ]
    }

    fn runs(grid: &GridIndex) -> Vec<(Option<Period>, usize, usize)> {
        grid.sections()
            .iter()
            .map(|s| (s.period, s.offset, s.count))
            .collect()
    }

    fn period(year: i64, month: Option<u32>, day: Option<u32>) -> Option<Period> {
        Some(Period { year, month, day })
    }

    /// A second apart across midnight on 31 December is two days, two months and two years;
    /// hours apart on one day is one of each. The folders the photos come from start nothing.
    #[test]
    fn a_period_layout_starts_a_section_where_the_period_changes() {
        let days = GridIndex::build(timeline(), Layout::Periods(PeriodUnit::Day));
        assert_eq!(
            runs(&days),
            [
                (period(2026, Some(1), Some(1)), 0, 1),
                (period(2025, Some(12), Some(31)), 1, 2),
                (period(2025, Some(11), Some(30)), 3, 1),
            ]
        );
        let months = GridIndex::build(timeline(), Layout::Periods(PeriodUnit::Month));
        assert_eq!(
            runs(&months),
            [
                (period(2026, Some(1), None), 0, 1),
                (period(2025, Some(12), None), 1, 2),
                (period(2025, Some(11), None), 3, 1),
            ]
        );
        let years = GridIndex::build(timeline(), Layout::Periods(PeriodUnit::Year));
        assert_eq!(
            runs(&years),
            [
                (period(2026, None, None), 0, 1),
                (period(2025, None, None), 1, 3)
            ]
        );
        assert!(days.sections().iter().all(|s| s.folder_id.is_none()));
        // The run's oldest photo, as for a folder: the 31st's is the one at 08:00.
        assert_eq!(days.sections()[1].taken_at_min, at(2025, 12, 31, 8, 0, 0));
        // The sidebar's folders are the same whatever the sections are.
        assert_eq!(
            days.folders(),
            GridIndex::build(timeline(), Layout::Folders).folders()
        );
    }

    /// Reversed, the timeline runs oldest first: still one section per period, none twice.
    #[test]
    fn a_reversed_timeline_has_each_period_once() {
        let mut rows = timeline();
        rows.reverse();
        let months = GridIndex::build(rows, Layout::Periods(PeriodUnit::Month));
        assert_eq!(
            runs(&months),
            [
                (period(2025, Some(11), None), 0, 1),
                (period(2025, Some(12), None), 1, 2),
                (period(2026, Some(1), None), 3, 1),
            ]
        );
    }

    /// No folder has a header under a period layout, so a jump lands on the first of the
    /// folder's photos, as in a flat one.
    #[test]
    fn a_folder_jump_under_periods_lands_on_the_folders_first_photo() {
        let grid = GridIndex::build(timeline(), Layout::Periods(PeriodUnit::Month));
        assert_eq!(grid.offset_of_folder(10), Some(0));
        assert_eq!(grid.offset_of_folder(20), Some(1));
        assert_eq!(grid.offset_of_folder(99), None);
    }

    /// A file with no capture date is dated by its modification time, which can be zero or
    /// negative: the second before the epoch is the last day of 1969, not a panic.
    #[test]
    fn a_date_before_1970_has_its_own_period() {
        assert_eq!(
            PeriodUnit::Day.of(-1),
            Period {
                year: 1969,
                month: Some(12),
                day: Some(31)
            }
        );
        assert_eq!(
            PeriodUnit::Year.of(0),
            Period {
                year: 1970,
                month: None,
                day: None
            }
        );
    }

    /// A search with no hits under a date grouping: nothing to head.
    #[test]
    fn an_empty_view_has_no_period_sections() {
        let grid = GridIndex::build(Vec::new(), Layout::Periods(PeriodUnit::Day));
        assert!(grid.sections().is_empty());
        assert_eq!(grid.offset_of_folder(10), None);
    }

    #[test]
    fn a_period_serialises_with_the_parts_it_has() {
        let grid = GridIndex::build(vec![entry_at(1, 10, 0)], Layout::Periods(PeriodUnit::Month));
        assert_eq!(
            serde_json::to_string(&grid.sections()[0]).unwrap(),
            r#"{"folderId":null,"offset":0,"count":1,"takenAtMin":0,"period":{"year":1970,"month":1,"day":null}}"#
        );
    }
}
