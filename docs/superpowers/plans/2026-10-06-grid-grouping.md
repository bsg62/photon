# Grid Grouping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A "Group by" control beside the sort that lays the grid out by folder (the default), day, month, year, or with no headers.

**Architecture:** The grouping is a third field of the existing `Sort` value, so it travels the sort's whole path (view state, epoch guard, rollback, `set_sort`, `GridInfo.sort`, the UI's change in flight) with no new command. Under a date grouping `Sort::arrange` re-sorts the view's rows newest first and `GridIndex::build` starts a section wherever the period changes; a `Section` carries its `period`, and the UI draws a header for any section with a folder or a period.

**Tech Stack:** Rust (`photon-core`, `photon-app`), Svelte 5 runes + TypeScript (`ui/`), vitest, criterion, xtask screenshots.

**Spec:** `docs/superpowers/specs/2026-10-06-photon-grid-grouping-design.md` — read it first; this plan argues from it.

## Global Constraints

- Read `CLAUDE.md` before starting. Its rules bind every task; the ones this work leans on:
- **The Rust gate before every commit that touches Rust**, in this order: `cargo fmt --all`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo bench -p photon-core --bench grid --no-run`.
- **The UI gate before every commit that touches `ui/`**, from the repo root: `npm run check` (0 errors and 0 warnings), `npm test`.
- **Every new test is demonstrated to fail with its change reverted.** Each task names the exact revert ("Probe"). A compile error is not proof. Undo a probe by editing the line back, never by copying a saved file over it (cargo then keeps the probed build; if you did, `touch` the file). If a probe *passes*, stop: find the input that would make the reverted code differ and add that case.
- **Never launch the GUI** (`npm run dev`) to verify. Verification is the suites, `svelte-check`, and the screenshot.
- **TypeScript mirrors are hand-written**: a Rust field and its `api.ts` counterpart change in the same task where the plan says so; test files are typechecked.
- Grouping applies only under the Date taken sort. Under Modified, Name and Size the grid is flat and the stored grouping is kept, not cleared.
- Under Day, Month, Year and None the order is newest first throughout; `reverse` turns it over.
- A period is computed in Rust with `metadata::civil_from_unix` (wall-clock reading). The UI formats a period from its numbers, never from a timestamp.
- The setting key is `grid_group`, values `folder`, `day`, `month`, `year`, `none`; anything else reads as `folder`. `grid_sort` keeps its form. No schema change.
- The control's option labels are "By folder", "By day", "By month", "By year", "No grouping": a closed select shows only its value, and a bare "Folder" beside "Date taken" does not say what it is.
- Comments carry reasoning, not mechanics; match the density and voice of the code around them.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.

## Review Focus

Inputs the spec implies and a person will meet. Each has its test in the task named.

1. **A stored grouping this photon does not know** (`week`, from a newer version or a hand edit): the library opens grouped by folder, not with an error. Tasks 2 and 3.
2. **Photos sharing one second** (a burst, a folder of undated scans copied at once) under a date grouping: they keep the same order on every rebuild instead of reshuffling. Task 2.
3. **A date before 1970** (a file with a zero or negative modification time): its period is the right day, with no panic. Task 1.
4. **An empty view under a date grouping** (a search with no hits): no sections, no header, no panic. Task 1.
5. **Grouping chosen, then the sort moved to Size and back**: the grouping is still there. It is stored alongside a non-date key and the sort control must not drop it. Tasks 3 and 4.

## File Structure

| File | Change |
| --- | --- |
| `crates/photon-core/src/grid.rs` | `PeriodUnit`, `Period`, `Layout::Periods`, `Section.period`, period sections in `build`, `offset_of_folder` |
| `crates/photon-core/src/sort.rs` | `Grouping`, `Sort.group`, `arrange` and `layout` under a grouping |
| `crates/photon-core/src/library/settings.rs` | `grid_group` read and written with `grid_sort` |
| `crates/photon-core/src/library/items.rs` | one integration test of `sorted_entries` |
| `crates/photon-app/src/engine.rs` | one integration test; literal fixes |
| `crates/photon-core/benches/grid.rs` | one bench case |
| `ui/src/lib/api.ts` | `Grouping`, `Period`, `Sort.group`, `Section.period` |
| `ui/src/lib/grouping.ts` (new) | the options, `groupingApplies`, `laidOutByFolder`, `periodLabel` |
| `ui/src/lib/layout.ts`, `timeline.ts`, `search.ts`, `folders.ts`, `library.svelte.ts` | read `period` / the grouping |
| `ui/src/components/GroupControl.svelte` (new), `Select.svelte`, `SortControl.svelte`, `Grid.svelte`, `FolderTree.svelte`, `ui/src/App.svelte` | the control and the header |
| `crates/xtask/screenshots/mock.js`, `crates/xtask/src/screenshots.rs` | a month-grouped shot |
| `CLAUDE.md`, `docs/smoke-checklist.md` | docs |

---

### Task 1: Period sections in the grid index

**Files:**
- Modify: `crates/photon-core/src/grid.rs`

**Interfaces:**
- Consumes: `crate::metadata::civil_from_unix(secs: i64) -> (i64, u32, u32)` (exists), `crate::metadata::naive_to_unix(year: i64, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64` (exists, `pub(crate)`, for tests).
- Produces:
  - `pub enum PeriodUnit { Day, Month, Year }` with `pub fn of(self, taken_at: i64) -> Period`
  - `pub struct Period { pub year: i64, pub month: Option<u32>, pub day: Option<u32> }` (`Copy`, `PartialEq`, `Eq`, `Serialize` camelCase)
  - `Layout::Periods(PeriodUnit)` beside `Folders` and `Flat`
  - `Section.period: Option<Period>`, the struct's **last** field

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `grid.rs`:

```rust
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
            [(period(2026, None, None), 0, 1), (period(2025, None, None), 1, 3)]
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
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p photon-core --lib grid`
Expected: does not compile (`PeriodUnit`, `Period`, `Layout::Periods`, `Section.period` unknown).

- [ ] **Step 3: Implement**

In `grid.rs`, replace the `Layout` enum and add the two types after it:

```rust
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
```

Replace the `Section` struct's doc and add the field (last):

```rust
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
```

In `GridIndex::build`, replace the `section_folder` closure and the section `match` with:

```rust
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
```

(The tally code below it, and the loop's closing brace, are unchanged.)

In `offset_of_folder`, change the flat arm to cover periods and extend the doc's first sentence:

```rust
    /// Where a jump to the folder lands: its header, or in a layout that gives folders no
    /// header (flat, or by period), the first of its photos the grid reaches.
```

```rust
            Layout::Flat | Layout::Periods(_) => {
                self.entries.iter().position(|e| e.folder_id == folder_id)
            }
```

Every existing `Section { .. }` literal in `grid.rs`'s tests gains `period: None`, and the existing `serialises_as_camel_case` expectation becomes `r#"{"folderId":1,"offset":2,"count":3,"takenAtMin":4,"period":null}"#`. The compiler names each literal (E0063).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p photon-core --lib grid`
Expected: PASS, all of `grid::tests`.

- [ ] **Step 5: Probe**

1. In `build`, change the `Periods` arm of `section_key` to `(None, None)`. Run `cargo test -p photon-core --lib grid`. Expected: `a_period_layout_starts_a_section_where_the_period_changes`, `a_reversed_timeline_has_each_period_once` and `a_period_serialises_with_the_parts_it_has` FAIL. Restore.
2. In `offset_of_folder`, move `Layout::Periods(_)` to the `Folders` arm (`Layout::Folders | Layout::Periods(_) => ...`). Expected: `a_folder_jump_under_periods_lands_on_the_folders_first_photo` FAILS. Restore.
3. In `PeriodUnit::of`, replace `civil_from_unix(taken_at)` with `civil_from_unix(taken_at.max(0))`. Expected: `a_date_before_1970_has_its_own_period` FAILS. Restore.

`an_empty_view_has_no_period_sections` has no probe: it pins that nothing panics on no rows, and no single line makes it differ. Say so in the commit message.

- [ ] **Step 6: Gate and commit**

Run the Rust gate (Global Constraints). Then:

```bash
git add crates/photon-core/src/grid.rs
git commit -m "feat(grid): sections by day, month or year

A Layout::Periods lays the index out as one section per period the rows
pass through, and a Section carries the period it is drawn under.

an_empty_view_has_no_period_sections has no failing probe: it pins that
an empty view does not panic, which no one line decides.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: The grouping in the sort

**Files:**
- Modify: `crates/photon-core/src/sort.rs`
- Modify (literals only): `crates/photon-core/src/library/settings.rs`, `crates/photon-core/src/library/items.rs`, `crates/photon-app/src/engine.rs`

**Interfaces:**
- Consumes: `grid::{Layout, PeriodUnit}` from Task 1.
- Produces:
  - `pub enum Grouping { Folder, Day, Month, Year, None }` (`Default` = `Folder`; serde camelCase: `"folder"`, `"day"`, `"month"`, `"year"`, `"none"`)
  - `Grouping::as_str(self) -> &'static str`, `Grouping::from_setting(stored: &str) -> Grouping`
  - `Sort { key: SortKey, reverse: bool, group: Grouping }`
  - `Sort::arrange` and `Sort::layout(view) -> Layout` honour `group`; signatures unchanged

- [ ] **Step 1: Write the failing tests**

In `sort.rs`'s `mod tests`, add a helper and the tests:

```rust
    const GROUPINGS: [Grouping; 5] = [
        Grouping::Folder,
        Grouping::Day,
        Grouping::Month,
        Grouping::Year,
        Grouping::None,
    ];

    fn dated(id: i64, folder_id: i64, taken_at: i64) -> GridEntry {
        GridEntry {
            folder_id,
            taken_at,
            ..entry(id, 0, 0)
        }
    }

    fn by(group: Grouping, reverse: bool) -> Sort {
        Sort {
            group,
            reverse,
            ..Sort::default()
        }
    }

    /// The view hands its rows over folder by folder. A date grouping is one timeline across
    /// them, newest first, and the four that are not the folder's agree on the order: they
    /// differ only in where the headers fall.
    #[test]
    fn a_date_grouping_is_one_timeline_newest_first_across_folders() {
        let view = || {
            vec![
                dated(1, 10, 100),
                dated(2, 10, 300),
                dated(3, 20, 200),
                dated(4, 20, 400),
            ]
        };
        let mut by_folder = view();
        by(Grouping::Folder, false).arrange(&mut by_folder, names);
        assert_eq!(ids(&by_folder), [1, 2, 3, 4]);
        for group in [Grouping::Day, Grouping::Month, Grouping::Year, Grouping::None] {
            let mut entries = view();
            by(group, false).arrange(&mut entries, names);
            assert_eq!(ids(&entries), [4, 2, 3, 1], "{group:?}");
            let mut reversed = view();
            by(group, true).arrange(&mut reversed, names);
            assert_eq!(ids(&reversed), [1, 3, 2, 4], "{group:?} reversed");
        }
    }

    /// A burst, or a folder of undated scans copied at once, shares one second. Those photos
    /// keep the view's order between them on every rebuild - and the list is long enough
    /// that the standard library's sort has left insertion sort, which is stable by accident.
    #[test]
    fn photos_of_one_second_keep_the_views_order_under_a_date_grouping() {
        for group in [Grouping::Day, Grouping::Month, Grouping::Year, Grouping::None] {
            let mut entries: Vec<GridEntry> = (0..200).map(|id| dated(id, 1, id % 3)).collect();
            by(group, false).arrange(&mut entries, names);
            for pair in entries.windows(2) {
                assert!(pair[0].taken_at >= pair[1].taken_at, "{group:?}: newest first");
                if pair[0].taken_at == pair[1].taken_at {
                    assert!(
                        pair[0].id < pair[1].id,
                        "{group:?}: {} before {}",
                        pair[0].id,
                        pair[1].id
                    );
                }
            }
        }
    }

    /// By name, size or modification time every photo is sorted together, whatever grouping
    /// is stored for the day the sort returns to the date.
    #[test]
    fn a_grouping_changes_nothing_under_another_key() {
        let fresh = || vec![entry(1, 10, 300), entry(2, 30, 100), entry(3, 20, 200)];
        for key in [SortKey::Modified, SortKey::Name, SortKey::Size] {
            let mut plain = fresh();
            Sort {
                key,
                ..Sort::default()
            }
            .arrange(&mut plain, names);
            for group in GROUPINGS {
                let sort = Sort {
                    key,
                    group,
                    ..Sort::default()
                };
                let mut entries = fresh();
                sort.arrange(&mut entries, names);
                assert_eq!(ids(&entries), ids(&plain), "{sort:?}");
                assert_eq!(sort.layout(GridView::All), Layout::Flat, "{sort:?}");
                assert_eq!(sort.layout(GridView::Recent), Layout::Flat, "{sort:?}");
            }
        }
    }

    /// By folder a view keeps its own layout, so Recent stays flat. A period lays every view
    /// out by that period, Recent included: it is already newest first. None is flat.
    #[test]
    fn the_grouping_decides_the_layout_under_the_date_sort() {
        use crate::grid::PeriodUnit;
        let layout = |group, view| by(group, false).layout(view);
        assert_eq!(layout(Grouping::Folder, GridView::All), Layout::Folders);
        assert_eq!(layout(Grouping::Folder, GridView::Recent), Layout::Flat);
        for view in [GridView::All, GridView::Starred, GridView::Recent] {
            assert_eq!(
                layout(Grouping::Day, view),
                Layout::Periods(PeriodUnit::Day),
                "{view:?}"
            );
            assert_eq!(
                layout(Grouping::Month, view),
                Layout::Periods(PeriodUnit::Month),
                "{view:?}"
            );
            assert_eq!(
                layout(Grouping::Year, view),
                Layout::Periods(PeriodUnit::Year),
                "{view:?}"
            );
            assert_eq!(layout(Grouping::None, view), Layout::Flat, "{view:?}");
        }
        assert_eq!(by(Grouping::Month, true).layout(GridView::All), Layout::Periods(PeriodUnit::Month));
    }

    /// A grouping a newer photon stored, or a hand-edited row, opens by folder.
    #[test]
    fn a_grouping_round_trips_and_falls_back_to_folder() {
        for group in GROUPINGS {
            assert_eq!(Grouping::from_setting(group.as_str()), group);
        }
        assert_eq!(Grouping::from_setting("week"), Grouping::Folder);
        assert_eq!(Grouping::from_setting(""), Grouping::Folder);
        assert_eq!(Grouping::from_setting("Month"), Grouping::Folder);
    }
```

Replace the existing `serialises_as_camel_case` test's body with:

```rust
        let sort = Sort {
            key: SortKey::Modified,
            reverse: true,
            group: Grouping::Month,
        };
        assert_eq!(
            serde_json::to_string(&sort).unwrap(),
            r#"{"key":"modified","reverse":true,"group":"month"}"#
        );
        assert_eq!(
            serde_json::from_str::<Sort>(r#"{"key":"date","reverse":false,"group":"none"}"#).unwrap(),
            by(Grouping::None, false)
        );
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p photon-core --lib sort`
Expected: does not compile (`Grouping` unknown, `Sort` has no field `group`).

- [ ] **Step 3: Implement**

In `sort.rs`, update the import and the module doc's second paragraph, then add `Grouping` after `SortKey`'s `impl`:

```rust
use crate::grid::{GridEntry, GridView, Layout, PeriodUnit};
```

Add to the module doc, after the paragraph beginning "Date is the library's own order":

```rust
//! By date the user also chooses where the headers fall (`Grouping`): at each folder, which
//! is that order, or at each day, month or year, or nowhere - one timeline across folders,
//! newest first, which is an order of its own and so part of the sort rather than beside it.
```

```rust
/// Where the grid's headers fall while it is sorted by date. It decides the order as well as
/// the headers, which is why it is a field of [`Sort`]: by folder the view's own order
/// stands, and every other choice is one timeline across folders, newest first. Under any
/// other key it is kept and ignored, so it is there again when the sort returns to the date.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Grouping {
    /// A header per folder: the grid photon has always had.
    #[default]
    Folder,
    Day,
    Month,
    Year,
    /// The timeline with no headers.
    None,
}

impl Grouping {
    /// The form stored in the settings table.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Folder => "folder",
            Self::Day => "day",
            Self::Month => "month",
            Self::Year => "year",
            Self::None => "none",
        }
    }

    /// Reads [`as_str`](Self::as_str)'s form. Anything else - a grouping a newer photon
    /// added, a hand-edited row - is the default rather than a library that fails to open.
    pub fn from_setting(stored: &str) -> Self {
        match stored {
            "day" => Self::Day,
            "month" => Self::Month,
            "year" => Self::Year,
            "none" => Self::None,
            _ => Self::Folder,
        }
    }
}
```

`Sort` gains the field (update its doc's first line to "A key, whether it runs backwards, and where the headers fall by date."):

```rust
pub struct Sort {
    pub key: SortKey,
    pub reverse: bool,
    pub group: Grouping,
}
```

Replace `Sort::layout`:

```rust
    /// How `view` is laid out under this sort. By any key but date it is flat: runs of one
    /// folder there are an accident of the key rather than sections. By date the grouping
    /// decides: by folder it is the view's own layout (so Recent, which interleaves folders,
    /// stays flat), a period lays every view out by that period, and none is flat.
    pub fn layout(self, view: GridView) -> Layout {
        if self.key != SortKey::Date {
            return Layout::Flat;
        }
        match self.group {
            Grouping::Folder => view.layout(),
            Grouping::Day => Layout::Periods(PeriodUnit::Day),
            Grouping::Month => Layout::Periods(PeriodUnit::Month),
            Grouping::Year => Layout::Periods(PeriodUnit::Year),
            Grouping::None => Layout::Flat,
        }
    }
```

In `Sort::from_setting`, the last line becomes:

```rust
        SortKey::parse(key).map_or_else(Self::default, |key| Self {
            key,
            reverse,
            group: Grouping::default(),
        })
```

and add to its doc: "The grouping is stored under a key of its own (`Library::grid_sort`), so this form is the one an older photon reads."

In `Sort::arrange`, replace the `SortKey::Date => {}` arm and extend the doc comment's first paragraph with "By date under a grouping other than the folder's, that is every photo newest first.":

```rust
            // By folder the view's own order is the answer. Otherwise the folders it
            // arrives grouped by give way to one timeline; the stable sort leaves photos of
            // one second in the view's order.
            SortKey::Date => {
                if self.group != Grouping::Folder {
                    entries.sort_by_key(|e| Reverse(e.taken_at));
                }
            }
```

Every other `Sort { key: .., reverse: .. }` literal in the workspace gains `..Sort::default()` as its last entry (the compiler names each, E0063): the remaining tests in `sort.rs`, `settings.rs:660`, `items.rs:3203`, and the six in `engine.rs`'s tests. `Sort { key, reverse }` becomes `Sort { key, reverse, ..Sort::default() }`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Probe**

1. In `arrange`, delete the `if self.group != Grouping::Folder { .. }` block's body (leave the arm empty). Run `cargo test -p photon-core --lib sort`. Expected: `a_date_grouping_is_one_timeline_newest_first_across_folders` and `photos_of_one_second_keep_the_views_order_under_a_date_grouping` FAIL. Restore.
2. In `arrange`, change `sort_by_key` to `sort_unstable_by_key`. Expected: `photos_of_one_second_...` FAILS. Restore.
3. In `arrange`, remove the `if` so the date sort runs under `Folder` too. Expected: `a_date_grouping_is_one_timeline...` FAILS on the `by_folder` assertion (and `date_leaves_the_views_order_and_reverse_turns_any_order_over` may too). Restore.
4. In `layout`, delete the `if self.key != SortKey::Date` early return. Expected: `a_grouping_changes_nothing_under_another_key` FAILS. Restore.
5. In `layout`, change the `Grouping::Folder` arm to `Layout::Folders`. Expected: `the_grouping_decides_the_layout_under_the_date_sort` FAILS on Recent. Restore.
6. In `Grouping::from_setting`, change `_ => Self::Folder` to `_ => Self::None`. Expected: `a_grouping_round_trips_and_falls_back_to_folder` FAILS. Restore.

- [ ] **Step 6: Gate and commit**

Run the Rust gate. Then:

```bash
git add crates/photon-core/src/sort.rs crates/photon-core/src/library/settings.rs crates/photon-core/src/library/items.rs crates/photon-app/src/engine.rs
git commit -m "feat(sort): a grouping, which by date decides the order and the layout

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: The setting, and the grouping end to end in the backend

**Files:**
- Modify: `crates/photon-core/src/library/settings.rs`
- Modify (tests): `crates/photon-core/src/library/items.rs`, `crates/photon-app/src/engine.rs`
- Modify: `crates/photon-core/benches/grid.rs`

**Interfaces:**
- Consumes: `Grouping`, `Sort.group`, `Grouping::as_str`, `Grouping::from_setting` (Task 2); `Section.period` (Task 1).
- Produces: `Library::grid_sort()` returns the stored grouping in `Sort.group`; `Library::set_grid_sort(sort)` stores it under `grid_group`. Signatures unchanged. `Engine::set_sort` needs no change: it already rebuilds with the whole `Sort` and stores it through `set_grid_sort`.

- [ ] **Step 1: Write the failing tests**

`settings.rs`, in `mod tests`:

```rust
    /// The grouping is stored beside the sort, under its own key, and with any key: chosen,
    /// then the sort moved to Size, it is there when the sort comes back to the date.
    #[test]
    fn the_grouping_is_stored_with_the_sort_and_falls_back_to_folder() {
        use crate::sort::{Grouping, SortKey};
        let (_dir, lib) = temp_library();
        assert_eq!(lib.grid_sort().unwrap().group, Grouping::Folder);
        let by_month = Sort {
            group: Grouping::Month,
            ..Sort::default()
        };
        lib.set_grid_sort(by_month).unwrap();
        assert_eq!(lib.grid_sort().unwrap(), by_month);
        // An older photon reads the sort it knows.
        assert_eq!(lib.setting(GRID_SORT).unwrap().as_deref(), Some("date"));

        let by_size_grouped = Sort {
            key: SortKey::Size,
            reverse: true,
            group: Grouping::Year,
        };
        lib.set_grid_sort(by_size_grouped).unwrap();
        assert_eq!(lib.grid_sort().unwrap(), by_size_grouped);

        lib.set_setting(GRID_GROUP, "week").unwrap();
        assert_eq!(lib.grid_sort().unwrap().group, Grouping::Folder);
        assert_eq!(lib.grid_sort().unwrap().key, SortKey::Size);
    }
```

`items.rs`, in `mod tests`, after `sorted_entries_order_every_photo_by_the_key_across_folders`:

```rust
    /// The view's query hands its rows over folder by folder - in a filtered view, with each
    /// folder placed by its oldest *matching* photo. Under a date grouping none of that order
    /// survives: every photo of the view, newest first.
    #[test]
    fn a_date_grouping_orders_a_view_newest_first_across_folders() {
        use crate::sort::{Grouping, Sort};
        let (_dir, lib) = temp_library();
        let (watched, old_folder) = seed_folder(&lib, Path::new("/p/old"));
        let new_folder = lib.upsert_folder(watched, None, "/p/new", 1).unwrap();
        let ids = lib
            .insert_items(&[
                new_item(old_folder, "/p/old/a.jpg", 1),
                new_item(old_folder, "/p/old/b.jpg", 20),
                new_item(new_folder, "/p/new/c.jpg", 10),
                new_item(old_folder, "/p/old/d.jpg", 30),
            ])
            .unwrap();
        // Starred: all but the newest.
        lib.set_ratings(&[(ids[0], 1), (ids[1], 1), (ids[2], 1)])
            .unwrap();
        let order = |view, group, reverse| -> Vec<i64> {
            let sort = Sort {
                group,
                reverse,
                ..Sort::default()
            };
            lib.sorted_entries(view, "", sort)
                .unwrap()
                .iter()
                .map(|e| e.id)
                .collect()
        };
        // By folder: `/p/new` first, its oldest photo being newer than `/p/old`'s.
        assert_eq!(
            order(GridView::All, Grouping::Folder, false),
            [ids[2], ids[0], ids[1], ids[3]]
        );
        for group in [Grouping::Day, Grouping::Month, Grouping::Year, Grouping::None] {
            assert_eq!(
                order(GridView::All, group, false),
                [ids[3], ids[1], ids[2], ids[0]],
                "{group:?}"
            );
            assert_eq!(
                order(GridView::All, group, true),
                [ids[0], ids[2], ids[1], ids[3]],
                "{group:?} reversed"
            );
            assert_eq!(
                order(GridView::Starred, group, false),
                [ids[1], ids[2], ids[0]],
                "{group:?} starred"
            );
        }
    }
```

`engine.rs`, in `mod tests`, after `a_sort_whose_rebuild_fails_is_rolled_back_and_not_remembered`:

```rust
    /// A grouping travels the sort's path: the rebuild lays the grid out by period, the
    /// layout generation moves (the sections changed, the folders did not), `GridInfo`
    /// reports it, and the next launch opens grouped the same way. Recent, flat by folder,
    /// takes the period headers too.
    #[test]
    fn a_grouping_lays_the_grid_out_by_period_and_is_remembered() {
        use photon_core::grid::GridView;
        use photon_core::sort::{Grouping, Sort};
        let img = jpeg(16, 16);
        let f = fixture(&[("a/one.jpg", &img), ("b/two.jpg", &img)]);
        f.add_photos();
        let (_, before, _, layout) = f.engine.published();
        assert!(
            before
                .sections()
                .iter()
                .all(|s| s.folder_id.is_some() && s.period.is_none())
        );
        let by_month = Sort {
            group: Grouping::Month,
            ..Sort::default()
        };
        let by_period = |grid: &GridIndex| {
            !grid.sections().is_empty()
                && grid
                    .sections()
                    .iter()
                    .all(|s| s.folder_id.is_none() && s.period.is_some_and(|p| p.day.is_none()))
        };

        f.engine.set_sort(by_month).unwrap();

        let (_, grid, _, moved) = f.engine.published();
        assert!(by_period(&grid));
        assert_eq!(grid.sections().iter().map(|s| s.count).sum::<usize>(), 2);
        assert_eq!(grid.folders(), before.folders(), "the same photos, the same folders");
        assert_eq!(moved, layout + 1);
        assert_eq!(crate::commands::grid_info(&f.engine, None).sort, by_month);

        f.engine.set_view(GridView::Recent).unwrap();
        assert!(by_period(&f.engine.grid().1));

        let reopened =
            Engine::open(f.config(), Arc::new(crate::events::Recorder::default())).unwrap();
        assert_eq!(reopened.sort(), by_month);
        reopened.startup(None);
        reopened.wait_for_startup();
        assert!(by_period(&reopened.grid().1));
        reopened.shutdown();
    }
```

(If `GridIndex` is not already in scope in that test module, add `use photon_core::grid::GridIndex;` inside the test.)

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p photon-core the_grouping_is_stored_with_the_sort && cargo test -p photon-app a_grouping_lays_the_grid_out_by_period`
Expected: the settings test does not compile (`GRID_GROUP` unknown). Temporarily comment that test out to see the other two: `a_date_grouping_orders_a_view_newest_first_across_folders` PASSES already (Task 2 made it true; it is an integration pin, probed in Step 5) and the engine test FAILS at `assert_eq!(reopened.sort(), by_month)`.

- [ ] **Step 3: Implement**

`settings.rs`: add the key beside `GRID_SORT`, import `Grouping` beside the existing `Sort` import, and replace the two functions:

```rust
/// Where the grid's headers fall by date (`sort::Grouping`). A key of its own rather than
/// more syntax in `grid_sort`, whose form an older photon reads: it finds the sort it knows
/// and never sees this.
const GRID_GROUP: &str = "grid_group";
```

```rust
    /// What the grid is sorted by, and how it is grouped by date; by date and by folder
    /// when never set.
    pub fn grid_sort(&self) -> Result<Sort> {
        let mut sort = self
            .setting(GRID_SORT)?
            .map_or_else(Sort::default, |stored| Sort::from_setting(&stored));
        if let Some(stored) = self.setting(GRID_GROUP)? {
            sort.group = Grouping::from_setting(&stored);
        }
        Ok(sort)
    }

    /// Stores what the grid is sorted by and how it is grouped. Two rows, written one after
    /// the other: a failure between them leaves a sort and a grouping that were each chosen,
    /// just not together, and either pair is one the grid can show.
    pub fn set_grid_sort(&self, sort: Sort) -> Result<()> {
        self.set_setting(GRID_SORT, &sort.to_setting())?;
        self.set_setting(GRID_GROUP, sort.group.as_str())
    }
```

`benches/grid.rs`: add `sort::{Grouping, Sort},` to the `photon_core::{..}` import, and in `bench_grid`, after the `startup_grid_100k` case:

```rust
    // The same library as one timeline with a header a month: the view's query, the
    // re-sort by capture date that a date grouping adds, and a period read per photo.
    let by_month = Sort {
        group: Grouping::Month,
        ..Sort::default()
    };
    c.bench_function("startup_grid_100k_by_month", |b| {
        b.iter(|| {
            black_box(GridIndex::build(
                lib.sorted_entries(GridView::All, "", by_month).unwrap(),
                by_month.layout(GridView::All),
            ))
        })
    });
```

(The spec asks for a number at 300k; this bench file's library is 100k throughout, so the case matches its neighbour `startup_grid_100k` and the two can be compared directly.)

- [ ] **Step 4: Run the tests and the bench**

Run: `cargo test --workspace`
Expected: PASS.

Run: `cargo bench -p photon-core --bench grid -- startup_grid_100k`
Expected: both `startup_grid_100k` and `startup_grid_100k_by_month` report a time. Record both numbers for the commit message. If the grouped build is more than twice the plain one, stop and report it rather than committing.

- [ ] **Step 5: Probe**

1. In `set_grid_sort`, delete the `GRID_GROUP` write (return the first call's result). Run `cargo test -p photon-core the_grouping_is_stored && cargo test -p photon-app a_grouping_lays_the_grid_out`. Expected: both FAIL. Restore.
2. In `grid_sort`, change `Grouping::from_setting(&stored)` to `Grouping::Month`. Expected: the settings test FAILS on the `"week"` assertion. Restore.
3. In `sort.rs`'s `arrange`, empty the date arm again (as Task 2's probe 1). Run `cargo test -p photon-core a_date_grouping_orders_a_view`. Expected: FAILS. Restore.
4. In `sort.rs`'s `layout`, make the `Grouping::Month` arm return `view.layout()`. Run `cargo test -p photon-app a_grouping_lays_the_grid_out`. Expected: FAILS at the first `by_period`. Restore.

- [ ] **Step 6: Gate and commit**

Run the Rust gate. Then (fill in the two measured times):

```bash
git add crates/photon-core/src/library/settings.rs crates/photon-core/src/library/items.rs crates/photon-app/src/engine.rs crates/photon-core/benches/grid.rs
git commit -m "feat(sort): the grouping is remembered, under grid_group

startup_grid_100k <plain> ms, startup_grid_100k_by_month <grouped> ms.

No test of a failed rebuild rolling a grouping back: the grouping is a
field of the Sort that a_sort_whose_rebuild_fails_is_rolled_back_and_not_remembered
already follows through that path, and a second copy would pass with or
without this change.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: The UI's types and pure modules

**Files:**
- Modify: `ui/src/lib/api.ts`, `ui/src/lib/layout.ts`, `ui/src/lib/timeline.ts`, `ui/src/lib/search.ts`, `ui/src/lib/folders.ts`, `ui/src/lib/library.svelte.ts` (the default literal only), `ui/src/components/SortControl.svelte`
- Create: `ui/src/lib/grouping.ts`, `ui/src/lib/grouping.test.ts`
- Test: `ui/src/lib/layout.test.ts`, `ui/src/lib/timeline.test.ts`, `ui/src/lib/search.test.ts`, `ui/src/lib/folders.test.ts`; literal fixes in `ui/src/lib/library.test.ts`

**Interfaces:**
- Consumes: the JSON of Tasks 1–3 — `Section.period` as `{ year, month, day } | null`, `Sort.group` as `'folder' | 'day' | 'month' | 'year' | 'none'`.
- Produces:
  - `api.ts`: `type Grouping`, `interface Period { year: number; month: number | null; day: number | null }`, `Sort.group: Grouping`, `Section.period: Period | null`
  - `grouping.ts`: `GROUPINGS: { value: Grouping; label: string }[]`, `groupingApplies(sort: Sort): boolean`, `laidOutByFolder(sort: Sort): boolean`, `periodLabel(period: Period, locale?: string): string`
  - `layout.ts`: `SectionLike.period?: Period | null`, `hasHeader(section: SectionLike): boolean`
  - `folders.ts`: `photoCount(count: number, locale?: string): string`

- [ ] **Step 1: Write the failing tests**

Create `ui/src/lib/grouping.test.ts`:

```ts
import { describe, expect, it } from 'vitest';
import type { Grouping, Sort } from './api';
import { GROUPINGS, groupingApplies, laidOutByFolder, periodLabel } from './grouping';

const sort = (key: Sort['key'], group: Grouping): Sort => ({ key, reverse: false, group });

describe('GROUPINGS', () => {
  it('offers every grouping once, the folder first', () => {
    expect(GROUPINGS.map((g) => g.value)).toEqual(['folder', 'day', 'month', 'year', 'none']);
  });
});

describe('groupingApplies', () => {
  it('only under the date sort, whatever grouping is stored', () => {
    expect(groupingApplies(sort('date', 'month'))).toBe(true);
    expect(groupingApplies(sort('date', 'none'))).toBe(true);
    for (const key of ['modified', 'name', 'size'] as const) expect(groupingApplies(sort(key, 'month'))).toBe(false);
  });
});

describe('laidOutByFolder', () => {
  it('is the date sort grouped by folder, and nothing else', () => {
    expect(laidOutByFolder(sort('date', 'folder'))).toBe(true);
    expect(laidOutByFolder({ key: 'date', reverse: true, group: 'folder' })).toBe(true);
    for (const group of ['day', 'month', 'year', 'none'] as const) expect(laidOutByFolder(sort('date', group))).toBe(false);
    // A flat sort keeps its stored grouping, and is not by folder for having it.
    expect(laidOutByFolder(sort('size', 'folder'))).toBe(false);
  });
});

describe('periodLabel', () => {
  it('names a year, a month and a day', () => {
    expect(periodLabel({ year: 2026, month: null, day: null }, 'en-US')).toBe('2026');
    expect(periodLabel({ year: 2026, month: 10, day: null }, 'en-US')).toBe('October 2026');
    expect(periodLabel({ year: 2026, month: 10, day: 4 }, 'en-US')).toBe('Sunday, October 4, 2026');
  });

  it('reads the numbers as they are, at either end of a year', () => {
    // Built from an instant instead, one of these lands in the neighbouring year in any
    // zone that is not UTC.
    expect(periodLabel({ year: 2025, month: 12, day: 31 }, 'en-US')).toBe('Wednesday, December 31, 2025');
    expect(periodLabel({ year: 2026, month: 1, day: 1 }, 'en-US')).toBe('Thursday, January 1, 2026');
    expect(periodLabel({ year: 2026, month: 1, day: null }, 'en-US')).toBe('January 2026');
  });
});
```

Append to `ui/src/lib/layout.test.ts` (add `hasHeader` to its import from `./layout`):

```ts
describe('period sections', () => {
  const month = (m: number, offset: number, count: number) => ({ folderId: null, period: { year: 2026, month: m, day: null }, offset, count });

  it('get a header each, as a folder does', () => {
    const rows = buildRows([month(10, 0, 3), month(9, 3, 2)], 2, TILE_WIDTH.medium);
    expect(rows.map((r) => r.kind)).toEqual(['header', 'tiles', 'tiles', 'header', 'tiles']);
    expect(rows[3].first).toBe(3);
  });

  it('and a run with neither a folder nor a period gets none', () => {
    const rows = buildRows([{ folderId: null, period: null, offset: 0, count: 3 }], 2, TILE_WIDTH.medium);
    expect(rows.map((r) => r.kind)).toEqual(['tiles', 'tiles']);
    expect(hasHeader({ folderId: null, offset: 0, count: 3 })).toBe(false);
    expect(hasHeader({ folderId: 7, offset: 0, count: 3 })).toBe(true);
  });
});
```

In `ui/src/lib/timeline.test.ts`, add `period: null` to the `section` helper's object, and add inside `describe('yearMarks', ...)`:

```ts
  it('reads a period section\'s own year, not the instant of its oldest photo', () => {
    // `takenAtMin` deliberately says another year: the period is what the header says.
    const period = (year: number, offset: number): Section => ({ folderId: null, offset, count: 1, takenAtMin: mid(2030), period: { year, month: 12, day: 31 } });
    const sections = [period(2026, 0), period(2025, 1), period(2025, 2)];
    expect(yearMarks(sections, buildRows(sections, 4, TILE_WIDTH.medium)).map((m) => m.year)).toEqual([2026, 2025]);
  });
```

In `ui/src/lib/search.test.ts`, inside `describe('viewKey', ...)`:

```ts
  it('so a new grouping resets the scroll by date, and changes nothing under another key', () => {
    const byMonth = { ...base, view: 'all' as const, sort: { key: 'date' as const, reverse: false, group: 'month' as const } };
    const byDay = { ...byMonth, sort: { ...byMonth.sort, group: 'day' as const } };
    expect(viewKey({ ...base, view: 'all' }).order).toBe('date');
    expect(resultsChanged(viewKey({ ...base, view: 'all' }), viewKey(byMonth))).toBe(true);
    expect(resultsChanged(viewKey(byMonth), viewKey(byDay))).toBe(true);
    expect(resultsChanged(viewKey(byMonth), viewKey({ ...byMonth }))).toBe(false);
    // By size the grid is flat whatever the grouping: the same list, the same place.
    const bySize = { ...base, view: 'all' as const, sort: { key: 'size' as const, reverse: false, group: 'folder' as const } };
    const bySizeGrouped = { ...bySize, sort: { ...bySize.sort, group: 'month' as const } };
    expect(resultsChanged(viewKey(bySize), viewKey(bySizeGrouped))).toBe(false);
  });
```

In `ui/src/lib/folders.test.ts`, add `photoCount` to the import from `./folders` and add:

```ts
describe('photoCount', () => {
  it('counts photos, and does not call one "photos"', () => {
    expect(photoCount(1, 'en-US')).toBe('1 photo');
    expect(photoCount(4210, 'en-US')).toBe('4,210 photos');
  });
});
```

- [ ] **Step 2: Run them to see them fail**

Run: `npm test -w ui -- src/lib/grouping.test.ts src/lib/layout.test.ts src/lib/timeline.test.ts src/lib/search.test.ts src/lib/folders.test.ts`
Expected: FAIL — `./grouping` does not exist, `hasHeader` and `photoCount` are not exported, the period header and year-mark and view-key cases fail.

- [ ] **Step 3: Implement**

`api.ts` — replace the `Section`, and `Sort` declarations and add the two types:

```ts
/** Mirrors `grid::Period`: a day, a month (`day` null) or a year (both null), as numbers.
 *  Never a timestamp: the UI names the day Rust put the photos in, and must not read an
 *  instant again in the viewer's own zone. */
export interface Period { year: number; month: number | null; day: number | null }
/** Mirrors `grid::Section`: a run the grid lays out. `folderId` names the folder a run is
 *  drawn under, `period` the day, month or year; a flat view's one run has neither and no
 *  header. `takenAtMin` is in SECONDS. */
export interface Section { folderId: number | null; offset: number; count: number; takenAtMin: number; period: Period | null }
```

```ts
/** Mirrors `sort::Grouping`. */
export type Grouping = 'folder' | 'day' | 'month' | 'year' | 'none';
/** Mirrors `sort::Sort`: what every view is sorted by. `date` keeps the headers `group`
 *  chooses - a folder's, a period's, or none; any other key lays the grid out flat and
 *  ignores `group`, which is kept for when the sort returns. `reverse` turns the whole
 *  order over. */
export interface Sort { key: SortKey; reverse: boolean; group: Grouping }
```

Create `ui/src/lib/grouping.ts`:

```ts
/** The grid's grouping (`sort::Grouping`), as the UI reads it. Pure. */

import type { Grouping, Period, Sort } from './api';

/** The control's options. Each label says what it is on its own: a closed select shows only
 *  its value, and a bare "Folder" beside "Date taken" does not. */
export const GROUPINGS: { value: Grouping; label: string }[] = [
  { value: 'folder', label: 'By folder' },
  { value: 'day', label: 'By day' },
  { value: 'month', label: 'By month' },
  { value: 'year', label: 'By year' },
  { value: 'none', label: 'No grouping' },
];

/** Whether the grouping decides anything: only by date. By name, size or modification time
 *  every photo is sorted together and the stored grouping waits for the sort to come back. */
export function groupingApplies(sort: Sort): boolean {
  return sort.key === 'date';
}

/** Whether the grid runs folder by folder, each under its header. The place photon
 *  remembers in the library is a folder, so it is a place in this arrangement only: under
 *  any other a jump to that folder lands on one of its photos, somewhere, not where the
 *  user was. */
export function laidOutByFolder(sort: Sort): boolean {
  return sort.key === 'date' && sort.group === 'folder';
}

/** What a period's header says: "2026", "October 2026", "Sunday, October 4, 2026". Built
 *  from the period's own numbers as a local date, which is then formatted as the same local
 *  date - so the zone cancels out, where a timestamp would be read back shifted by it. */
export function periodLabel(period: Period, locale?: string): string {
  if (period.month === null) return String(period.year);
  const date = new Date(period.year, period.month - 1, period.day ?? 1);
  return period.day === null
    ? date.toLocaleDateString(locale, { month: 'long', year: 'numeric' })
    : date.toLocaleDateString(locale, { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' });
}
```

`layout.ts` — import `Period` beside `Section`, extend `SectionLike`, add `hasHeader`, use it in `buildRows`:

```ts
import type { Period, Section } from './api';
```

```ts
export interface SectionLike {
  /** Null for a run drawn under no folder: a period's, or a flat view's. */
  folderId: number | null;
  /** The day, month or year a run is drawn under, when the grid is grouped by one. */
  period?: Period | null;
  offset: number;
  count: number;
}
```

```ts
/** A section gets a header row when it names a folder or a period; a flat view's run names
 *  neither. */
export function hasHeader(section: SectionLike): boolean {
  return section.folderId !== null || (section.period ?? null) !== null;
}
```

In `buildRows`, replace its doc comment's sentence and the condition `if (s.folderId !== null) {` with `if (hasHeader(s)) {` (doc: "A section gets a header row when `hasHeader` says so.").

`timeline.ts` — in `yearMarks`, replace `const year = yearOf(section.takenAtMin);` with:

```ts
    // A period section says its year itself, in the reading Rust grouped by; a folder's is
    // the year of its oldest photo, as the sidebar files it.
    const year = section.period?.year ?? yearOf(section.takenAtMin);
```

and in the module doc change "It reads each section's `takenAtMin`" to "It reads each folder section's `takenAtMin`" and append "A period section carries its own year."

`search.ts` — in `viewKey`, replace the `return` with:

```ts
  const { key, reverse, group } = info.sort;
  // The grouping reorders the grid only by date. Under another key it is ignored, and a
  // change to it must not throw the scroll position away.
  const grouped = key === 'date' && group !== 'folder' ? `:${group}` : '';
  return { view: info.view, query, order: `${reverse ? '-' : ''}${key}${grouped}` };
```

and extend the function's doc: "- plus the sort, which reorders every view, and by date the grouping, which does too."

`folders.ts` — add `photoCount` above `folderSummary` and use it there:

```ts
/** "1 photo", "4,210 photos": what a header says it holds. */
export function photoCount(count: number, locale?: string): string {
  return count === 1 ? '1 photo' : `${count.toLocaleString(locale)} photos`;
}
```

```ts
export function folderSummary(count: number, takenAtMin: number, locale?: string): string {
  const month = new Date(takenAtMin * 1000).toLocaleDateString(locale, { month: 'long', year: 'numeric' });
  return `${photoCount(count, locale)} · ${month}`;
}
```

`library.svelte.ts` — the default: `sort: { key: 'date', reverse: false, group: 'folder' },`.

`SortControl.svelte` — the two changes keep whatever else the sort holds (a hand-built `{ key, reverse }` would drop the grouping, and no longer typechecks):

```svelte
  <Select label="Sort by" options={KEYS} value={sort.key} onchange={(key) => library.setSort({ ...sort, key })} />
```

```svelte
    onclick={() => library.setSort({ ...sort, reverse: !sort.reverse })}
```

Then the literals the typecheck now refuses:

```bash
sed -i "s/reverse: \(false\|true\) }/reverse: \1, group: 'folder' as const }/g" ui/src/lib/library.test.ts ui/src/lib/folders.test.ts ui/src/lib/search.test.ts
sed -i "s/takenAtMin: \([^,}]*\) }/takenAtMin: \1, period: null }/g" ui/src/lib/library.test.ts
npm run check
```

Fix by hand whatever `npm run check` still names (a literal the patterns missed, or one they should not have touched — a folder tally is not a section and takes no `period`).

- [ ] **Step 4: Run the tests**

Run: `npm run check && npm test`
Expected: 0 errors, 0 warnings; all tests PASS.

- [ ] **Step 5: Probe**

1. `grouping.ts`, `laidOutByFolder`: return `sort.key === 'date'`. Expected: its test FAILS. Restore.
2. `grouping.ts`, `groupingApplies`: return `true`. Expected: its test FAILS. Restore.
3. `grouping.ts`, `periodLabel`: build the date as `new Date(Date.UTC(period.year, period.month - 1, period.day ?? 1))`. Run with `TZ=America/Los_Angeles npm test -w ui -- src/lib/grouping.test.ts`. Expected: the day and month labels FAIL (a day early). Restore. (In a UTC zone this probe passes; that is why it is run with `TZ` set.)
4. `layout.ts`, `hasHeader`: return `section.folderId !== null`. Expected: "get a header each" FAILS. Restore.
5. `timeline.ts`: restore `const year = yearOf(section.takenAtMin);`. Expected: the period year-mark test FAILS. Restore.
6. `search.ts`: set `grouped` to `''`. Expected: the grouping view-key test FAILS on its second assertion. Then set it to `` `:${group}` `` unconditionally: FAILS on `order).toBe('date')` and on the by-size assertion. Restore.

`photoCount` is an extraction: its test and the existing `folderSummary` tests fail together if its body is broken (make it return `''` to see).

- [ ] **Step 6: Gate and commit**

Run the UI gate. Then:

```bash
git add ui/src
git commit -m "feat(ui): sections with a period, and the grouping in the sort

The mirror of grid::Period and sort::Grouping, a header for a period
section, year marks from periods, and a view key that a grouping change
moves by date and not under another key.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Select-all and the place in the library

**Files:**
- Modify: `ui/src/lib/library.svelte.ts` (`selectAll`), `ui/src/lib/folders.ts` (`returnToAll`), `ui/src/components/FolderTree.svelte`, `ui/src/components/Grid.svelte` (the restore's gate)
- Test: `ui/src/lib/library.test.ts`, `ui/src/lib/folders.test.ts`

**Interfaces:**
- Consumes: `hasHeader(section)` from `layout.ts`, `laidOutByFolder(sort)` from `grouping.ts` (Task 4).
- Produces: `returnToAll`'s dependency `sortedByDate` is renamed `byFolder: () => boolean`.

- [ ] **Step 1: Write the failing tests**

In `ui/src/lib/library.test.ts`, beside `takes the lead photo's folder under a flat sort, wherever its photos are`:

```ts
      it('takes the lead photo\'s month under a date grouping: what is under its header', async () => {
        const months = [
          { folderId: null, offset: 0, count: 5, takenAtMin: 0, period: { year: 2026, month: 10, day: null } },
          { folderId: null, offset: 5, count: 10, takenAtMin: 0, period: { year: 2026, month: 9, day: null } },
        ];
        const store = await storeOf(15, { sections: months });
        store.selected = 7;
        vi.mocked(api.gridFolderIdsAt).mockClear();

        await store.selectAll();

        // A range, like a folder's section - not the folder's scattered ids.
        expect(api.gridFolderIdsAt).not.toHaveBeenCalled();
        expect(store.selectionCount).toBe(10);
        expect(store.isSelected(idAt(5))).toBe(true);
        expect(store.isSelected(idAt(14))).toBe(true);
        expect(store.isSelected(idAt(4))).toBe(false);
        expect(store.selected).toBe(7);
      });
```

In `ui/src/lib/folders.test.ts`, rename the existing case and its dependency:

```ts
  it('opens at the top unless the grid runs folder by folder, the only order a folder is a place in', async () => {
    const { order, jumped, deps } = spyDeps('starred');
    await returnToAll({ ...deps, byFolder: () => false });
    expect(order).toEqual(['cancel', 'setView:all']);
    expect(jumped).toEqual([]);
  });
```

and in `spyDeps` rename the `sortedByDate` property it builds to `byFolder` (same value).

- [ ] **Step 2: Run them to see them fail**

Run: `npm test -w ui -- src/lib/library.test.ts src/lib/folders.test.ts`
Expected: the month select-all FAILS (`selectionCount` 0: the period section's null `folderId` sends it to the folder path, which the mock answers with nothing). The `returnToAll` case FAILS or fails to typecheck on `byFolder`.

- [ ] **Step 3: Implement**

`library.svelte.ts` — import `hasHeader` from `./layout` (extend the existing import) and replace the head of `selectAll`:

```ts
  async selectAll(): Promise<void> {
    // All with no headers (a flat sort, or grouped by nothing): still the folder being
    // looked at, but its photos are scattered through the list, so it is a set of ids
    // rather than a range. Under a header - a folder's or a period's - it is the section.
    const first = this.info.sections[0];
    if (this.info.view === 'all' && first && !hasHeader(first)) {
      return this.selectFolderAt(this.selectedOffset ?? 0);
    }
```

Update `selectAll`'s doc comment's first sentence to: "Ctrl/Cmd+A: selects what is under the lead's header - its folder, or under a date grouping its day, month or year - or, outside the library view, every photo in the view." and in `selectAllRange`'s doc change "the unit the user is actually looking at is one folder" to "the unit the user is actually looking at is one section, a folder or a period".

`folders.ts` — in `returnToAll`'s `deps`, replace the `sortedByDate` member and its doc, and its one use:

```ts
  /** Whether the grid runs folder by folder (`laidOutByFolder`). The remembered folder is a
   *  place in that arrangement, written only while All is in it; under a flat sort or a
   *  date grouping a jump would land on that folder's first photo wherever the order put it
   *  - somewhere, not where the user was - so All opens at its top, as it does at launch
   *  (`Grid.svelte`'s restore). */
  byFolder: () => boolean;
```

```ts
  const folderId = deps.byFolder() ? await deps.lastFolder().catch(() => null) : null;
```

`FolderTree.svelte` — import `laidOutByFolder` from `'../lib/grouping'` and in `showAll`:

```ts
      byFolder: () => laidOutByFolder(library.info.sort),
```

`Grid.svelte` — import `laidOutByFolder` from `'../lib/grouping'` and replace the restore's guard and its comment:

```ts
        // The folder remembered is a place in the folder order, written only while All is
        // laid out by folder. Under a flat sort or a date grouping a jump would still land -
        // on the folder's first photo wherever the order put it - which is somewhere, not
        // where the user left off.
        if (!laidOutByFolder(library.info.sort)) return;
```

(The write, the effect below it, needs no change: `topFolderId` answers null for a section with no folder, as it does under a flat sort today. No test is added for that: it would pass with and without this branch.)

- [ ] **Step 4: Run the tests**

Run: `npm run check && npm test`
Expected: 0 errors, 0 warnings; PASS.

- [ ] **Step 5: Probe**

1. `library.svelte.ts`: restore the old condition `this.info.sections[0]?.folderId === null`. Expected: the month select-all FAILS. Restore.
2. Change the condition to `first && hasHeader(first)` (inverted). Expected: `takes the lead photo's folder under a flat sort` FAILS. Restore.

The restore's gate in `Grid.svelte` is effect wiring with no harness (`CLAUDE.md`, "There is no component test harness"): the predicate is tested in Task 4, the wiring is on the smoke checklist (Task 7). Say so in the commit message.

- [ ] **Step 6: Gate and commit**

Run the UI gate. Then:

```bash
git add ui/src
git commit -m "feat(grid): Ctrl+A takes the period under a date grouping; the place in the library is a folder order's

The launch restore's gate in Grid.svelte is effect wiring, which has no
test harness here: laidOutByFolder is tested, the wiring is on the smoke
checklist.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: The control and the header

**Files:**
- Create: `ui/src/components/GroupControl.svelte`
- Modify: `ui/src/components/Select.svelte`, `ui/src/App.svelte`, `ui/src/components/Grid.svelte`

**Interfaces:**
- Consumes: `GROUPINGS`, `groupingApplies`, `periodLabel` (`grouping.ts`), `photoCount` (`folders.ts`), `library.sort`, `library.setSort(sort: Sort)`.
- Produces: `<GroupControl />`; `Select`'s new optional prop `disabled?: boolean`.

There is no component test harness, so this task has no vitest step: it is verified by `svelte-check`, by the screenshot in Task 7, and by the smoke checklist. The commit message says so.

- [ ] **Step 1: `Select` can be disabled**

In `Select.svelte`, add the prop (in both the destructuring and the type):

```ts
    disabled = false,
```

```ts
    /** Shown, focusable and named, but not openable: for a choice that does not apply right
     *  now and will again, where hiding it would move its neighbours. */
    disabled?: boolean;
```

Guard the two ways in. At the top of `onkeydown`:

```ts
    if (disabled) return;
```

On the field, add `class:disabled`, `aria-disabled={disabled}`, and change the click:

```svelte
    onclick={() => !disabled && select.toggle()}
```

In the `<style>`, after the `.field:hover, .field.open` rule:

```css
  /* Spelled with the hover state so a disabled field does not light up under the pointer. */
  .field.disabled, .field.disabled:hover { background: var(--field); color: var(--text-dim); cursor: default; }
```

- [ ] **Step 2: `GroupControl`**

Create `ui/src/components/GroupControl.svelte`:

```svelte
<script lang="ts">
  import { GROUPINGS, groupingApplies } from '../lib/grouping';
  import { library } from '../lib/library.svelte';
  import Select from './Select.svelte';

  const sort = $derived(library.sort);
  const applies = $derived(groupingApplies(sort));
</script>

<!-- Holds no grouping of its own, like the sort control beside it: the grouping is a field
     of `library.sort`, so a change here builds on a sort change still in flight and a
     refused one puts both controls back.

     Disabled rather than hidden under a sort that ignores it: hidden, the top bar's controls
     would shift whenever the sort changed, and the choice it still holds - the one that
     comes back with Date taken - would be out of sight. -->
<div class="group" title={applies ? undefined : 'Grouping applies when sorted by date taken'}>
  <Select label="Group by" options={GROUPINGS} value={sort.group} disabled={!applies} onchange={(group) => library.setSort({ ...sort, group })} />
</div>

<style>
  /* `flex: 0 0 auto` for the same reason as the controls beside it: the search bar is the
     top bar's one child meant to give way. */
  .group { display: inline-flex; flex: 0 0 auto; }
</style>
```

- [ ] **Step 3: Mount it**

In `App.svelte`, import it beside `SortControl` and place it between the sort and the size:

```ts
  import GroupControl from './components/GroupControl.svelte';
```

```svelte
      <SortControl />
      <GroupControl />
      <SizeControl />
```

- [ ] **Step 4: The period header**

In `Grid.svelte`, import `periodLabel` from `'../lib/grouping'` and add `photoCount` to the import from `'../lib/folders'`. Replace the header block's contents:

```svelte
          <div class="header" style:top="{row.top - shift}px">
            {#if section.period}
              <!-- A period names itself and belongs to no folder, so there is no path to
                   show; the count is the section's, as a folder's is. -->
              <span class="name">{periodLabel(section.period)}</span>
              <span class="summary">{photoCount(section.count)}</span>
            {:else}
              <span class="name">{folder ? folderLabel(folder) : ''}</span>
              <!-- From the section, not the folder: it is there before the folder list is,
                   and it counts the photos under this header, which in a search are fewer. -->
              <span class="summary">{folderSummary(section.count, section.takenAtMin)}</span>
              <span class="path">{folder?.path ?? ''}</span>
            {/if}
          </div>
```

Update the comment on `const sections = $derived(library.info.sections);` to begin: "The index's own sections: one per folder run, one per day, month or year under a date grouping, or a single headerless run in a flat view (Recent)." and the year strip's comment to "It needs headers to mark (so a flat view, which has none, never shows it)".

- [ ] **Step 5: Verify**

Run: `npm run check && npm test`
Expected: 0 errors, 0 warnings; PASS.

- [ ] **Step 6: Commit**

```bash
git add ui/src
git commit -m "feat(ui): a Group by control, and headers for a day, a month and a year

No test: the control and the header are markup and one derived value,
and there is no component harness. svelte-check covers the types, the
screenshot and the smoke checklist the look.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: The screenshot, the docs, and the whole-branch check

**Files:**
- Modify: `crates/xtask/screenshots/mock.js`, `crates/xtask/src/screenshots.rs`, `CLAUDE.md`, `docs/smoke-checklist.md`

**Interfaces:**
- Consumes: everything above. No new IPC command, so `mock.js`'s `canned`/`SILENT` lists and the test that reads them are untouched.
- Produces: the shot `main-by-month-light`.

- [ ] **Step 1: The mock answers a grouped grid**

In `mock.js`, add `period: null` to each of the four `sections` literals and to the `sections.push({ .. })` in the `HUGE` branch. Change the echoed sort and add the grouped sections beside it:

```js
  // The sort the UI last set, echoed back the same way: the control reads it from grid_info.
  // ?group=<Grouping> is what it starts as.
  let sort = { key: 'date', reverse: false, group: P.get('group') || 'folder' };
  // By month the four folder runs are shown as four months, which is what each happens to
  // be. The photos are not re-sorted and the other groupings are not drawn: this is for the
  // header's look, and the real order is the backend's.
  const monthOf = (t) => {
    const d = new Date(t * 1000);
    return { year: d.getUTCFullYear(), month: d.getUTCMonth() + 1, day: null };
  };
  const shownSections = () =>
    sort.key === 'date' && sort.group === 'month' ? sections.map((s) => ({ ...s, folderId: null, period: monthOf(s.takenAtMin) })) : sections;
```

and in `grid_info`'s `layout`, send them (the `folders` line still maps the folder runs):

```js
        sections: shownSections(),
```

Add to the header comment's list of query parameters: "`?group=<Grouping>` the grouping the sort starts with".

- [ ] **Step 2: The shot**

In `screenshots.rs`, add to `SHOTS` after `main-dark`:

```rust
    // The grid as one timeline with a header a month, and the Group control saying so.
    Shot {
        name: "main-by-month-light",
        query: "theme=light&group=month",
        dark: false,
    },
```

If `screenshots.rs` or its tests state the number of shots anywhere, raise it by one (`grep -n "37\|thirty" crates/xtask/src/screenshots.rs`).

Run: `cargo test -p xtask`
Expected: PASS.

Run: `cargo run -p xtask -- screenshots --only main-by-month-light` (needs Chromium; if none is on `PATH` or in `CHROMIUM`, say so in the final report and skip the look).
Expected: `target/screenshots/main-by-month-light.png`. Open it with the Read tool and check: four headers reading "July 2026", "March 2026", "December 2025", "May 2025", each followed by its photo count and no path; the top bar shows "Date taken", then "By month", then the sizes; the year strip shows 2026 and 2025. Also run `--only main-light --no-build` and check the folder headers and the top bar ("By folder") look as before apart from the new control.

- [ ] **Step 3: `CLAUDE.md`**

Change "thirty-seven" to "thirty-eight" in both places (the Commands block and the Styling section).

In the Architecture section, after the paragraph beginning "**The user's sort**", add:

```markdown
**The grouping** (`sort::Grouping`, the `grid_group` setting) is a field of that `Sort`, not a
value beside it: by date it decides the order as well as the headers. `Folder` is the view's own
order and layout; `Day`, `Month` and `Year` are one timeline across folders, newest first
(`arrange`: a stable sort by `taken_at` over the view's rows), laid out as `Layout::Periods`,
in every view, Recent included; `None` is that timeline flat. Under any other key it is kept and
ignored, so the sort control changes a `Sort` by spreading it - built from its two fields, it
dropped the grouping. A `Section` carries its `period` as numbers (`grid::Period`, read with
`civil_from_unix` like search's `2024-06`), and the UI formats those (`periodLabel`), never
`takenAtMin`, which it would read again in the viewer's zone. A section has a header when it
has a folder or a period (`hasHeader`); code that means "flat" asks that, not
`folderId === null`, which a period section has too - `selectAll` did, and under a month header
selected a folder's scattered photos. The place photon remembers in the library is a folder, so
its restore and `returnToAll` ask `laidOutByFolder`, not the sort key.
```

- [ ] **Step 4: The smoke checklist**

In `docs/smoke-checklist.md`, after the item beginning "Changing the sort with text in the search box", add:

```markdown
- [ ] Group by (the menu between the sort and the photo sizes), sorted by Date taken: **By month**
      shows one timeline across folders, newest photo first, with a header at each month
      ("October 2026 · 312 photos") and no path; **By day** and **By year** show the same photos
      in the same order with headers at each day ("Sunday, 4 October 2026" in the system's
      format) and each year; **No grouping** shows them with no headers and no year strip;
      **By folder** is the grid as it was. The year strip works under day, month and year.
      The reverse button turns each over, oldest first.
- [ ] A photo taken late on the last evening of a month is under that month's header, and a
      search for that month (`2026-09`) shows the same photos as the header counts.
- [ ] Sort by Name, Size or Date modified: the Group menu is dimmed, does not open, and says
      why on hover; the grid is flat as before. Back on Date taken, the grouping chosen before
      is there again. Change the sort and the grouping quickly one after the other: both land.
- [ ] Grouped by month, open Starred, an album, a search and Recent: each is laid out by month.
      Grouped by folder, Recent has no headers, as before.
- [ ] Grouped by month in All, select a photo and press Ctrl+A: that month is selected. With
      No grouping, Ctrl+A selects that photo's folder, wherever its photos sit.
- [ ] Grouped by month, click a folder in the sidebar: the grid scrolls to a photo of that
      folder. Open a photo: the caption counts within the month ("3 / 312").
- [ ] Choose a grouping, quit and relaunch: photon opens grouped the same way, at the top.
      Back on By folder, quit and relaunch: the grid opens at the folder last browsed.
- [ ] Changing the grouping scrolls the grid to the top and clears the selection.
```

- [ ] **Step 5: Both gates, in full**

Run the Rust gate and the UI gate from Global Constraints, then `cargo run -p xtask -- metadata` and `cargo run -p xtask -- versions`.
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add crates/xtask CLAUDE.md docs/smoke-checklist.md
git commit -m "docs: the grouping in CLAUDE.md and the smoke checklist; a month-grouped screenshot

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 7: An independent read of the whole branch**

`CLAUDE.md`: "A large branch gets an independent read before it merges." Give a fresh reviewer the diff `main...feat/grid-grouping` and the spec, and point them at what the feature *arms* in old code rather than only at the new code:

- Every reader of `Section.folderId` / `section.folder_id` and of `info.sort.key`: does any still mean "flat" or "by folder" by a test that a period section or a grouped date sort now answers wrongly? (`grep -rn "folderId\b" ui/src`, `grep -rn "sort.key" ui/src`, `grep -rn "folder_id.is_none\|Layout::Flat" crates`.)
- `Grid.svelte`'s effects: the restore, the last-folder write, the scroll-to-top on a view-key change, the year strip - under each grouping and across a grouping change.
- The viewer: `positionInSection` under a period section, and `orphaned`/re-find by id after a grouping change shifts every offset.
- `library.setSort` callers: is any `Sort` still built from fields rather than spread?
- A sort change and a grouping change in flight together (`LibraryStore.sort`, `requestedSort`).

Fix what the review finds with a test where one can exist, re-run both gates, and commit.

---

## Self-review notes

- **Spec coverage.** Semantics: Tasks 1–3 (order, layout, Recent, wall-clock periods, the setting). "What follows the headers": year strip (Task 4), viewer counter (unchanged code, counted within the section; smoke item), Ctrl+A (Task 5), sidebar (unchanged; `offset_of_folder` in Task 1), the place in the library (Tasks 4–5), scroll to top and cleared selection (Task 4's view key; `setSort` already clears the selection). UI and control: Task 6. Screenshot and docs: Task 7. Bench: Task 3.
- **One deviation from the spec:** the bench case runs at 100k, the size of the library that bench file builds, not 300k.
- **One wording choice beyond the spec:** the option labels ("By month"), under Global Constraints.
