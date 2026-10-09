//! A section's header: a folder's name, how many of its photos the view holds and since
//! when, and its path; or a period's name and count. Drawn in its row and, once that row
//! has scrolled away, pinned over the top of the grid - by this one function, so the two
//! cannot come to say different things.

use super::{
    labels::{folder_label, folder_summary, period_label, photo_count},
    layout::HEADER,
};
use crate::text::paint_line;
use crate::theme::{
    apply::color,
    fonts,
    tokens::{Palette, S, T},
};
use eframe::egui::{self, Color32, FontId, Rect};
use jiff::tz::TimeZone;
use photon_core::{grid::Section, library::Folder};
use std::collections::HashMap;

/// What a header says, left to right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    pub name: String,
    pub summary: String,
    /// A folder's path. Empty for a period, which belongs to no folder.
    pub path: String,
}

/// The heading of `section`. A folder not in `folders` yet - the list is read again a
/// moment after the grid that names it - has a blank name and no path, and its count, which
/// comes from the section.
pub fn heading(section: &Section, folders: &HashMap<i64, Folder>, zone: &TimeZone) -> Heading {
    if let Some(period) = section.period {
        return Heading {
            name: period_label(period),
            summary: photo_count(section.count),
            path: String::new(),
        };
    }
    let folder = section.folder_id.and_then(|id| folders.get(&id));
    Heading {
        name: folder.map(folder_label).unwrap_or_default().to_owned(),
        // From the section, not the folder: it counts the photos under this header, which
        // in a search are fewer.
        summary: folder_summary(section.count, section.taken_at_min, zone),
        path: folder.map(|folder| folder.path.clone()).unwrap_or_default(),
    }
}

/// Paints `heading` in `rect`, which is `HEADER` tall. `pinned` gives it the surface behind
/// and the line under it that set it off from the rows it lies over.
pub fn paint(ui: &egui::Ui, rect: Rect, heading: &Heading, pinned: bool, palette: &Palette) {
    let painter = ui.painter_at(rect);
    if pinned {
        painter.rect_filled(rect, 0.0, color(palette.surface));
        painter.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            (1.0, color(palette.line)),
        );
    }
    // One baseline for three sizes: the name's line box is the row less its top padding,
    // and the smaller texts are centred on the same line.
    let middle = rect.top() + 7.0 + (HEADER as f32 - 7.0) / 2.0;
    let right = rect.right() - S[1];
    // Draws `text` from `left`, in at most `share` of the room that is left, and answers
    // where the next text starts.
    let text = |left: f32, text: &str, font: FontId, tint: Color32, share: f32| -> f32 {
        let room = (right - left) * share;
        if text.is_empty() || room <= 0.0 {
            return left;
        }
        left + paint_line(ui, &painter, (left, middle), room, text, font, tint) + S[2]
    };
    // The name may take 70% of the row, so a long one cannot squeeze the path to nothing.
    let left = rect.left() + S[1];
    let dim = color(palette.text_dim);
    let name = fonts::semibold(ui.ctx(), T[3]);
    let left = text(left, &heading.name, name, color(palette.text), 0.7);
    let left = text(left, &heading.summary, fonts::regular(T[1]), dim, 1.0);
    text(left, &heading.path, fonts::regular(T[0]), dim, 1.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use photon_core::grid::Period;

    fn folders() -> HashMap<i64, Folder> {
        let folder = Folder {
            id: 7,
            watched_id: 1,
            parent_id: None,
            path: "/photos/2026-07 Coast".to_owned(),
            name: "2026-07 Coast".to_owned(),
            hidden: false,
            alias: Some("Coast".to_owned()),
        };
        HashMap::from([(7, folder)])
    }

    fn section(folder_id: Option<i64>, period: Option<Period>) -> Section {
        Section {
            folder_id,
            offset: 0,
            count: 23,
            // 2026-07-01 00:30 UTC.
            taken_at_min: 1_782_865_800,
            period,
        }
    }

    #[test]
    fn a_folders_header_names_it_counts_it_and_shows_its_path() {
        assert_eq!(
            heading(&section(Some(7), None), &folders(), &TimeZone::UTC),
            Heading {
                name: "Coast".to_owned(),
                summary: "23 photos · July 2026".to_owned(),
                path: "/photos/2026-07 Coast".to_owned(),
            }
        );
    }

    // The folder list is read by a task and the grid is not: a section can name a folder
    // the list does not hold yet.
    #[test]
    fn a_folder_not_listed_yet_has_its_count_and_a_blank_name() {
        let heading = heading(&section(Some(99), None), &folders(), &TimeZone::UTC);
        assert_eq!(heading.name, "");
        assert_eq!(heading.path, "");
        assert_eq!(heading.summary, "23 photos · July 2026");
    }

    #[test]
    fn a_periods_header_names_the_period_and_has_no_path() {
        let june = Period {
            year: 2024,
            month: Some(6),
            day: None,
        };
        assert_eq!(
            heading(&section(None, Some(june)), &folders(), &TimeZone::UTC),
            Heading {
                name: "June 2024".to_owned(),
                summary: "23 photos".to_owned(),
                path: String::new(),
            }
        );
    }
}
