//! The rows at the top of the sidebar: the views every library has, with their counts.
//! `FolderTree.svelte`'s first block, as a list the view draws.
//!
//! No egui here: a row is what it says and whether it is where the user is.

use crate::nav::{Place, Step};
use photon_core::grid::GridView;

/// A row's height, in points. Every row of the sidebar is this tall, which is what lets
/// the list be drawn a window at a time.
pub const ROW: f32 = 28.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fixed {
    All,
    Starred,
    Recent,
    OnThisDay,
    Videos,
    Duplicates,
    /// The photo whose copies are shown, while they are: under Duplicates, and not a
    /// button - the view is already open, and leaving it removes the row.
    CopiesOf,
    Hidden,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub what: Fixed,
    pub label: String,
    pub count: Option<usize>,
    /// Whether this is where the user is, or is going.
    pub active: bool,
    /// What the row says when the pointer rests on it.
    pub hint: String,
}

/// What the library holds of each kind (`grid_info`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub starred: usize,
    pub duplicates: usize,
    pub hidden: usize,
    pub videos: usize,
    /// The file name of the photo whose copies are shown, while the view is Copies.
    pub copies_of: Option<String>,
}

/// Today, as "On this day" means it: the machine's own day - where the user is - while a
/// capture date is the camera's wall clock, so the two meet as plain calendar days with no
/// zone between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Today {
    pub month: u32,
    pub day: u32,
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

impl Today {
    /// The search for photos taken on this day in any year, in `search::Query`'s grammar.
    pub fn query(self) -> String {
        format!("on:{:02}-{:02}", self.month, self.day)
    }

    /// "14 July", for the row's hint.
    pub fn label(self) -> String {
        let month = MONTHS
            .get(self.month.saturating_sub(1) as usize)
            .copied()
            .unwrap_or_default();
        format!("{} {month}", self.day)
    }
}

impl Fixed {
    /// What a click on the row asks for, or nothing for the row that is not a button.
    pub fn step(self, today: Today) -> Option<Step> {
        Some(match self {
            Fixed::All => Step::View(GridView::All),
            Fixed::Starred => Step::View(GridView::Starred),
            Fixed::Recent => Step::View(GridView::Recent),
            Fixed::OnThisDay => Step::Search(today.query()),
            Fixed::Videos => Step::View(GridView::Videos),
            Fixed::Duplicates => Step::View(GridView::Duplicates),
            Fixed::Hidden => Step::View(GridView::Hidden),
            Fixed::CopiesOf => return None,
        })
    }
}

/// The rows, top to bottom, for a library holding `counts` and a user at `at`.
///
/// Videos, Duplicates and Hidden are there only while they hold something or are what is
/// shown: most libraries have no duplicates, and a permanent "0" row is noise.
pub fn fixed_rows(counts: &Counts, at: &Place, today: Today) -> Vec<Row> {
    let view = at.view;
    let row = |what: Fixed, label: &str, count: Option<usize>, active: bool, hint: &str| Row {
        what,
        label: label.to_owned(),
        count,
        active,
        hint: hint.to_owned(),
    };
    let mut rows = vec![
        row(
            Fixed::All,
            "All photos",
            None,
            view == GridView::All,
            "Every photo, back where you left the gallery",
        ),
        row(
            Fixed::Starred,
            "Starred",
            Some(counts.starred),
            view == GridView::Starred,
            "Starred photos",
        ),
        row(
            Fixed::Recent,
            "Recent",
            None,
            view == GridView::Recent,
            "The newest photos by capture date",
        ),
        // A search, not a view, and no count, as a saved search has none: it would mean
        // running the search on every change to the library.
        row(
            Fixed::OnThisDay,
            "On this day",
            None,
            *at == Place::search(&today.query()),
            &format!("Photos taken on {}, in any year", today.label()),
        ),
    ];
    if counts.videos > 0 || view == GridView::Videos {
        rows.push(row(
            Fixed::Videos,
            "Videos",
            Some(counts.videos),
            view == GridView::Videos,
            "Every video in the library",
        ));
    }
    if counts.duplicates > 0 || matches!(view, GridView::Duplicates | GridView::Copies) {
        rows.push(row(
            Fixed::Duplicates,
            "Duplicates",
            Some(counts.duplicates),
            view == GridView::Duplicates,
            "Photos with a byte-identical copy elsewhere in the library",
        ));
        if view == GridView::Copies {
            let name = counts.copies_of.as_deref().filter(|name| !name.is_empty());
            rows.push(row(
                Fixed::CopiesOf,
                &format!("Copies of {}", name.unwrap_or("a photo")),
                None,
                true,
                name.unwrap_or_default(),
            ));
        }
    }
    if counts.hidden > 0 || view == GridView::Hidden {
        rows.push(row(
            Fixed::Hidden,
            "Hidden",
            Some(counts.hidden),
            view == GridView::Hidden,
            "Photos you have hidden. They stay on disk; unhide them from here",
        ));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: Today = Today { month: 7, day: 4 };

    fn kinds(rows: &[Row]) -> Vec<Fixed> {
        rows.iter().map(|row| row.what).collect()
    }

    #[test]
    fn a_library_of_photos_alone_has_the_four_rows_every_library_has() {
        let rows = fixed_rows(&Counts::default(), &Place::of(GridView::All), TODAY);
        assert_eq!(
            kinds(&rows),
            [Fixed::All, Fixed::Starred, Fixed::Recent, Fixed::OnThisDay]
        );
        // Starred is counted even at none; All photos and Recent are not counted at all.
        assert_eq!(rows[1].count, Some(0));
        assert_eq!(rows[0].count, None);
        assert_eq!(rows[2].count, None);
        let active: Vec<bool> = rows.iter().map(|row| row.active).collect();
        assert_eq!(active, [true, false, false, false]);
    }

    #[test]
    fn videos_duplicates_and_hidden_are_there_while_they_hold_something() {
        let counts = Counts {
            starred: 12,
            duplicates: 3,
            hidden: 240,
            videos: 7,
            copies_of: None,
        };
        let rows = fixed_rows(&counts, &Place::of(GridView::Starred), TODAY);
        assert_eq!(
            kinds(&rows),
            [
                Fixed::All,
                Fixed::Starred,
                Fixed::Recent,
                Fixed::OnThisDay,
                Fixed::Videos,
                Fixed::Duplicates,
                Fixed::Hidden
            ]
        );
        let counted: Vec<Option<usize>> = rows.iter().map(|row| row.count).collect();
        assert_eq!(
            counted,
            [None, Some(12), None, None, Some(7), Some(3), Some(240)]
        );
        assert!(rows[1].active);
        assert_eq!(rows.iter().filter(|row| row.active).count(), 1);
    }

    // The last hidden photo unhidden while Hidden is shown: the row the user is on must
    // not vanish from under them.
    #[test]
    fn an_empty_one_stays_while_it_is_what_is_shown() {
        for (view, what) in [
            (GridView::Videos, Fixed::Videos),
            (GridView::Duplicates, Fixed::Duplicates),
            (GridView::Hidden, Fixed::Hidden),
        ] {
            let rows = fixed_rows(&Counts::default(), &Place::of(view), TODAY);
            let row = rows.iter().find(|row| row.what == what).unwrap();
            assert!(row.active, "{what:?}");
            assert_eq!(row.count, Some(0));
        }
    }

    #[test]
    fn the_copies_of_a_photo_are_a_row_under_duplicates_that_is_not_a_button() {
        let counts = Counts {
            copies_of: Some("IMG_0042.jpg".to_owned()),
            ..Counts::default()
        };
        let at = Place {
            view: GridView::Copies,
            arg: "42".to_owned(),
        };
        let rows = fixed_rows(&counts, &at, TODAY);
        let position = rows
            .iter()
            .position(|row| row.what == Fixed::CopiesOf)
            .unwrap();
        assert_eq!(rows[position - 1].what, Fixed::Duplicates);
        assert!(
            !rows[position - 1].active,
            "Duplicates is not what is shown"
        );
        assert_eq!(rows[position].label, "Copies of IMG_0042.jpg");
        assert!(rows[position].active);
        assert_eq!(Fixed::CopiesOf.step(TODAY), None);
        // A photo that has gone has no name left to read.
        let rows = fixed_rows(&Counts::default(), &at, TODAY);
        assert!(rows.iter().any(|row| row.label == "Copies of a photo"));
    }

    #[test]
    fn on_this_day_is_a_search_for_todays_date_in_any_year() {
        assert_eq!(TODAY.query(), "on:07-04");
        assert_eq!(TODAY.label(), "4 July");
        assert_eq!(Today { month: 12, day: 25 }.query(), "on:12-25");
        assert_eq!(
            Fixed::OnThisDay.step(TODAY),
            Some(Step::Search("on:07-04".to_owned()))
        );
        let rows = fixed_rows(&Counts::default(), &Place::search("on:07-04"), TODAY);
        let row = rows
            .iter()
            .find(|row| row.what == Fixed::OnThisDay)
            .unwrap();
        assert!(row.active);
        assert_eq!(row.hint, "Photos taken on 4 July, in any year");
        // Another search is not this row's, and yesterday's is not today's.
        let rows = fixed_rows(&Counts::default(), &Place::search("on:07-03"), TODAY);
        assert!(rows.iter().all(|row| !row.active));
    }

    #[test]
    fn each_row_asks_for_its_own_view() {
        assert_eq!(Fixed::All.step(TODAY), Some(Step::View(GridView::All)));
        assert_eq!(
            Fixed::Starred.step(TODAY),
            Some(Step::View(GridView::Starred))
        );
        assert_eq!(
            Fixed::Recent.step(TODAY),
            Some(Step::View(GridView::Recent))
        );
        assert_eq!(
            Fixed::Videos.step(TODAY),
            Some(Step::View(GridView::Videos))
        );
        assert_eq!(
            Fixed::Duplicates.step(TODAY),
            Some(Step::View(GridView::Duplicates))
        );
        assert_eq!(
            Fixed::Hidden.step(TODAY),
            Some(Step::View(GridView::Hidden))
        );
    }
}
