//! The sidebar, drawn: the rows of `rows.rs`, top to bottom, and which of them was clicked.

use super::rows::{Fixed, ROW, Row};
use crate::{
    grid::labels::grouped,
    icons::Icon,
    text::paint_line,
    theme::{
        apply::{color, palette},
        fonts,
        tokens::{R, S, T},
    },
};
use eframe::egui::{self, Align2, Rect, Sense, WidgetInfo, WidgetType, pos2};

/// What a row is inset by from the panel's edges.
const INSET: f32 = 6.0;
const ICON: f32 = 14.0;

fn icon(what: Fixed) -> Option<Icon> {
    Some(match what {
        Fixed::All => Icon::LayoutGrid,
        Fixed::Starred => Icon::Star,
        Fixed::Recent => Icon::Clock,
        Fixed::OnThisDay => Icon::Calendar,
        Fixed::Videos => Icon::Play,
        Fixed::Duplicates => Icon::Copy,
        Fixed::Hidden => Icon::EyeOff,
        Fixed::CopiesOf => return None,
    })
}

/// Draws the sidebar in `rect` and answers the row that was clicked.
pub fn show(ui: &mut egui::Ui, rect: Rect, rows: &[Row]) -> Option<Fixed> {
    let palette = palette(ui.ctx());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, color(palette.chrome));

    let mut clicked = None;
    let mut top = rect.top() + S[1];
    for row in rows {
        let place = Rect::from_min_max(
            pos2(rect.left() + INSET, top),
            pos2(rect.right() - INSET, top + ROW),
        );
        top += ROW;
        if place.top() >= rect.bottom() {
            break;
        }
        // The row that names the photo whose copies are shown does nothing on a click -
        // the view is already open - so it must not offer one.
        let button = row.what != Fixed::CopiesOf;
        let sense = if button {
            Sense::click()
        } else {
            Sense::hover()
        };
        let mut response = ui.interact(place.intersect(rect), ui.id().with(row.what), sense);
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, button, &row.label));
        if !row.hint.is_empty() {
            response = response.on_hover_text(&row.hint);
        }
        if button && response.clicked() {
            clicked = Some(row.what);
        }

        if row.active {
            painter.rect_filled(place, R[2], color(palette.accent_soft));
        } else if button && response.hovered() {
            painter.rect_filled(place, R[2], color(palette.hover));
        }
        let middle = place.center().y;
        let text = color(palette.text);
        let mut left = place.left() + S[1];
        match icon(row.what) {
            Some(icon) => {
                let at =
                    Rect::from_min_size(pos2(left, middle - ICON / 2.0), egui::Vec2::splat(ICON));
                icon.paint(ui, at, ICON, false, text);
                left += ICON + S[1];
            }
            // No icon: the name starts where a row under a group starts.
            None => left = place.left() + 28.0,
        }
        let mut right = place.right() - S[1];
        if let Some(count) = row.count {
            // Dimmed, except over the fill of the row that is shown, where the dimmed
            // colour does not have the contrast.
            let tint = if row.active {
                text
            } else {
                color(palette.text_dim)
            };
            let drawn = painter.text(
                pos2(right, middle),
                Align2::RIGHT_CENTER,
                grouped(count),
                fonts::regular(T[0]),
                tint,
            );
            right = drawn.left() - S[1];
        }
        paint_line(
            ui,
            &painter,
            (left, middle),
            right - left,
            &row.label,
            fonts::regular(T[2]),
            text,
        );
    }
    clicked
}
