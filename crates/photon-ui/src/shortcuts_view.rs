//! The list of shortcuts as it is drawn - `ShortcutList.svelte` - in Settings' Shortcuts
//! section, one column, and in the sheet `?` opens, two (`ShortcutSheet.svelte`): each
//! group a heading over its rows, a row its chords - each key a chip, a chord's keys joined
//! by a "+" - in a column of their own beside what they do. The sheet is a dialog of the
//! dialog layer (`dialog_view.rs`), and its body is scrolled as Settings' section is.

use crate::{
    dialog_view::{Done, close_labelled},
    settings_view::{Column, Page, header, line_height, wrap},
    shortcuts::{Group, SHEET_TITLE, SHORTCUTS, Shortcut, chord_label},
    theme::{
        apply::color,
        fonts,
        tokens::{Palette, R, S, T},
    },
};
use eframe::egui::{
    self, Id, Key, Modifiers, Rect, Sense, Stroke, StrokeKind, UiBuilder, WidgetInfo, WidgetType,
    pos2, vec2,
};

/// The column of keys beside what they do: narrow enough that the descriptions keep to
/// one line in the sheet's two columns. A row of keys wider than this wraps.
const KEYS: f32 = 152.0;
/// A key's chip: its words and this much room either side, at least this wide.
const CHIP_PAD: f32 = 6.0;
const CHIP_MIN: f32 = 21.0;
/// Between two chords of one row, and the room a row has over and under it.
const CHORD_GAP: f32 = S[1];
const ROW_PAD: f32 = 3.0;
/// The sheet: at most this wide, the two columns this far apart.
const SHEET_WIDTH: f32 = 820.0;
const COLUMN_GAP: f32 = S[5];

/// Whether the window is a Mac's: `Mod` is spelt ⌘ there.
fn mac() -> bool {
    cfg!(target_os = "macos")
}

/// Where each key of a row's chords goes, from the row's top left, and how tall they come
/// to: a chord's keys stay on one line, and the chords wrap within the column.
struct Placed {
    chips: Vec<(Rect, String)>,
    pluses: Vec<egui::Pos2>,
    height: f32,
}

fn chip_width(ui: &egui::Ui, key: &str) -> f32 {
    let words =
        ui.painter()
            .layout_no_wrap(key.to_owned(), fonts::regular(T[2]), egui::Color32::WHITE);
    (words.size().x.ceil() + 2.0 * CHIP_PAD).max(CHIP_MIN)
}

fn place_keys(ui: &egui::Ui, row: &Shortcut) -> Placed {
    let chip = line_height(ui, &fonts::regular(T[2])) + 2.0;
    let plus = ui
        .painter()
        .layout_no_wrap("+".to_owned(), fonts::regular(T[2]), egui::Color32::WHITE)
        .size()
        .x
        .ceil()
        + 4.0;
    let (mut x, mut y) = (0.0, 0.0);
    let mut placed = Placed {
        chips: Vec::new(),
        pluses: Vec::new(),
        height: chip,
    };
    for chord in row.keys {
        let keys = chord_label(chord, mac());
        let widths: Vec<f32> = keys.iter().map(|key| chip_width(ui, key)).collect();
        let whole = widths.iter().sum::<f32>() + plus * (keys.len() - 1) as f32;
        if x > 0.0 && x + whole > KEYS {
            x = 0.0;
            y += chip + S[0];
        }
        for (n, (key, width)) in keys.iter().zip(&widths).enumerate() {
            if n > 0 {
                placed.pluses.push(pos2(x + plus / 2.0, y + chip / 2.0));
                x += plus;
            }
            let rect = Rect::from_min_size(pos2(x, y), vec2(*width, chip));
            placed.chips.push((rect, (*key).to_owned()));
            x += width;
        }
        x += CHORD_GAP;
        placed.height = y + chip;
    }
    placed
}

/// How tall a row is in a column `width` wide: its keys, or what it does wrapped beside
/// them, whichever is taller.
fn row_height(ui: &egui::Ui, row: &Shortcut, width: f32) -> f32 {
    let keys = place_keys(ui, row).height;
    let does = wrap(
        ui,
        row.does,
        fonts::regular(T[2]),
        egui::Color32::WHITE,
        (width - KEYS - S[2]).max(1.0),
    );
    keys.max(does.size().y) + 2.0 * ROW_PAD
}

fn group_height(ui: &egui::Ui, group: &Group, width: f32) -> f32 {
    let heading = line_height(ui, &fonts::semibold(ui.ctx(), T[3])) + S[0];
    let rows: f32 = group
        .rows
        .iter()
        .map(|row| row_height(ui, row, width))
        .sum();
    heading + rows + S[3]
}

/// One group at the page's place, in its column.
fn draw_group(ui: &egui::Ui, page: &mut Page, group: &Group, palette: &Palette) {
    let heading_font = fonts::semibold(ui.ctx(), T[3]);
    let line = line_height(ui, &heading_font);
    let heading = Rect::from_min_size(pos2(page.left, page.y), vec2(page.width, line));
    if page.sees(heading) {
        page.painter.text(
            heading.left_center(),
            egui::Align2::LEFT_CENTER,
            group.title,
            heading_font,
            color(palette.text),
        );
        ui.interact(
            heading,
            Id::new(("shortcut-group", group.title)),
            Sense::hover(),
        )
        .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, group.title));
    }
    page.y = heading.bottom() + S[0];
    for row in group.rows {
        let height = row_height(ui, row, page.width);
        let rect = Rect::from_min_size(pos2(page.left, page.y), vec2(page.width, height));
        page.y = rect.bottom();
        if !page.sees(rect) {
            continue;
        }
        let top = rect.top() + ROW_PAD;
        let placed = place_keys(ui, row);
        for (chip, key) in &placed.chips {
            let chip = chip.translate(vec2(rect.left(), top));
            page.painter.rect_filled(chip, R[1], color(palette.field));
            page.painter.rect_stroke(
                chip,
                R[1],
                Stroke::new(1.0, color(palette.line)),
                StrokeKind::Inside,
            );
            page.painter.text(
                chip.center(),
                egui::Align2::CENTER_CENTER,
                key,
                fonts::regular(T[2]),
                color(palette.text),
            );
        }
        for plus in &placed.pluses {
            page.painter.text(
                *plus + vec2(rect.left(), top),
                egui::Align2::CENTER_CENTER,
                "+",
                fonts::regular(T[2]),
                color(palette.text_dim),
            );
        }
        let does_left = rect.left() + KEYS + S[2];
        let does = wrap(
            ui,
            row.does,
            fonts::regular(T[2]),
            color(palette.text_dim),
            (rect.right() - does_left).max(1.0),
        );
        page.painter
            .galley(pos2(does_left, top), does, color(palette.text_dim));
        // Told as one thing: its keys, then what they do.
        let keys: Vec<String> = (row.keys.iter())
            .map(|chord| chord_label(chord, mac()).join("+"))
            .collect();
        let label = format!("{}: {}", keys.join(" or "), row.does);
        ui.interact(
            rect,
            Id::new(("shortcut", group.title, row.does)),
            Sense::hover(),
        )
        .widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &label));
    }
    page.y += S[3];
}

/// The groups at the page's place: in one column, or in two, the groups in their order
/// and split where the two columns come out nearest the same height. A group is read as
/// one block, never split over the two.
pub(crate) fn draw_list(ui: &egui::Ui, page: &mut Page, wide: bool, palette: &Palette) {
    if !wide {
        for group in SHORTCUTS {
            draw_group(ui, page, group, palette);
        }
        return;
    }
    let width = (page.width - COLUMN_GAP) / 2.0;
    let heights: Vec<f32> = (SHORTCUTS.iter())
        .map(|group| group_height(ui, group, width))
        .collect();
    let total: f32 = heights.iter().sum();
    let split = (0..=SHORTCUTS.len())
        .min_by(|a, b| {
            let tallest = |k: &usize| {
                let left: f32 = heights[..*k].iter().sum();
                left.max(total - left)
            };
            tallest(a).total_cmp(&tallest(b))
        })
        .unwrap_or(SHORTCUTS.len());
    let (top, left) = (page.y, page.left);
    page.width = width;
    for group in &SHORTCUTS[..split] {
        draw_group(ui, page, group, palette);
    }
    let left_bottom = page.y;
    page.y = top;
    page.left = left + width + COLUMN_GAP;
    for group in &SHORTCUTS[split..] {
        draw_group(ui, page, group, palette);
    }
    page.y = page.y.max(left_bottom);
    page.left = left;
}

/// The sheet's size in `window`: its width, and the height its list needs or the window
/// has, whichever is less.
pub(crate) fn sheet_size(ui: &egui::Ui, window: Rect) -> egui::Vec2 {
    let width = SHEET_WIDTH.min(window.width() - 2.0 * S[3]).max(0.0);
    let inner = width - 2.0 * S[3];
    let column = (inner - COLUMN_GAP) / 2.0;
    let heights: Vec<f32> = (SHORTCUTS.iter())
        .map(|group| group_height(ui, group, column))
        .collect();
    let total: f32 = heights.iter().sum();
    let tallest = (0..=heights.len())
        .map(|k| {
            let left: f32 = heights[..k].iter().sum();
            left.max(total - left)
        })
        .fold(f32::MAX, f32::min);
    let height = crate::settings_view::HEADER_HEIGHT + S[3] + tallest;
    vec2(width, height.min(window.height() - 2.0 * S[3]).max(0.0))
}

/// What the sheet keeps from frame to frame: how far its list is scrolled.
#[derive(Default)]
pub struct SheetView {
    column: Column,
}

impl SheetView {
    /// The sheet in `rect`, the dialog's whole ground. `?` closes what it opened, as
    /// Escape does - the dialog's - and its cross.
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, rect: Rect, palette: &Palette) -> Done {
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Questionmark))
            || ui.input_mut(|input| input.consume_key(Modifiers::SHIFT, Key::Questionmark))
        {
            return Done::Closed;
        }
        let (header, cross) = header(ui, rect, SHEET_TITLE, palette);
        let place = Rect::from_min_max(pos2(rect.left(), header.bottom()), rect.max);
        let inner = Rect::from_min_max(
            pos2(place.left() + S[3], place.top() + S[3]),
            pos2(place.right() - S[3], place.bottom()),
        );
        let mut page = self.column.begin(ui, place, inner);
        let page = ui
            .scope_builder(UiBuilder::new().max_rect(inner), |ui| {
                ui.set_clip_rect(inner.intersect(ui.clip_rect()));
                draw_list(ui, &mut page, true, palette);
                page
            })
            .inner;
        self.column.end(ui, &page, place, inner, palette);
        if close_labelled(ui, cross, palette, "Close") {
            return Done::Closed;
        }
        Done::Open
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        dialog_view::{Done, tests::Fixture},
        dialogs::Dialog,
    };
    use eframe::egui::{Key, Modifiers};

    // The sheet: its title and its groups in two columns, the groups in their order and
    // none split. `?` closes what it opened, as Escape does.
    #[test]
    fn the_sheet_lists_the_groups_in_two_columns_and_question_mark_closes_it() {
        let mut f = Fixture::new(Dialog::Shortcuts);
        f.open();
        assert!(f.drew("Keyboard shortcuts").is_some());
        let everywhere = f.drew("Everywhere").unwrap();
        let video = f.drew("Video").unwrap();
        assert!(video.left() > everywhere.right(), "two columns");
        assert!(f.drew("Show this list").is_some());
        assert_eq!(
            f.key_with(Key::Questionmark, Modifiers::SHIFT),
            Done::Closed
        );
        let mut f = Fixture::new(Dialog::Shortcuts);
        f.open();
        assert_eq!(f.key(Key::Escape), Done::Closed);
    }
}
