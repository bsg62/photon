//! One tile: a square, the photo covering it, and its marks.

use super::labels::format_duration;
use crate::{
    icons::Icon,
    theme::{
        apply::color,
        fonts,
        tokens::{PHOTO_LINE, Palette, R, T},
    },
};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, TextureHandle, Vec2, pos2, vec2};
use photon_core::{grid::GridEntry, media::MediaKind};

/// How far a mark stands in from the tile's edges.
const INSET: f32 = 5.0;
const MARK: f32 = 14.0;
const PLAY: f32 = 12.0;
const PROBLEM: f32 = 28.0;

/// The part of a `width` by `height` picture that covers a square, centred: what
/// `object-fit: cover` shows. In texture coordinates, 0 to 1.
pub fn cover_uv(width: f32, height: f32) -> Rect {
    if width <= 0.0 || height <= 0.0 {
        return Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    }
    if width > height {
        let shown = height / width;
        Rect::from_min_max(
            pos2((1.0 - shown) / 2.0, 0.0),
            pos2((1.0 + shown) / 2.0, 1.0),
        )
    } else {
        let shown = width / height;
        Rect::from_min_max(
            pos2(0.0, (1.0 - shown) / 2.0),
            pos2(1.0, (1.0 + shown) / 2.0),
        )
    }
}

/// Paints `entry`'s tile in `rect`. `texture` is its thumbnail when it has arrived, and
/// `failed` whether it never will.
pub fn paint(
    ui: &egui::Ui,
    rect: Rect,
    entry: &GridEntry,
    texture: Option<&TextureHandle>,
    failed: bool,
    palette: &Palette,
) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, R[1], color(palette.field));
    if let Some(texture) = texture {
        let size = texture.size_vec2();
        egui::Image::from_texture(texture)
            .uv(cover_uv(size.x, size.y))
            .corner_radius(R[1])
            .paint_at(ui, rect);
    }
    let video = entry.kind == MediaKind::Video;
    if failed {
        // A video with no frame reads as a video, not as broken: the play glyph is what
        // it will look like once it has one.
        let icon = if video {
            Icon::Play
        } else {
            Icon::TriangleAlert
        };
        icon.paint(ui, rect, PROBLEM, false, color(palette.text_dim));
    }
    let on_photo = color(PHOTO_LINE);
    if video {
        let corner = rect.min + Vec2::splat(INSET);
        let play = Rect::from_min_size(corner, Vec2::splat(PLAY));
        Icon::Play.paint(ui, play, PLAY, true, on_photo);
        if let Some(ms) = entry.duration_ms {
            painter.text(
                pos2(play.right() + 3.0, play.center().y),
                Align2::LEFT_CENTER,
                format_duration(ms),
                fonts::regular(T[0]),
                on_photo,
            );
        }
    }
    if entry.starred {
        let corner = rect.max - Vec2::splat(INSET + MARK);
        mark(ui, Icon::Star, corner, true, color(palette.star));
    }
    if entry.has_copies {
        let corner = pos2(rect.left() + INSET, rect.bottom() - INSET - MARK);
        mark(ui, Icon::Copy, corner, false, on_photo);
    }
}

fn mark(ui: &egui::Ui, icon: Icon, corner: Pos2, filled: bool, tint: Color32) {
    icon.paint(
        ui,
        Rect::from_min_size(corner, vec2(MARK, MARK)),
        MARK,
        filled,
        tint,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uv(width: f32, height: f32) -> (f32, f32, f32, f32) {
        let rect = cover_uv(width, height);
        (rect.min.x, rect.min.y, rect.max.x, rect.max.y)
    }

    #[test]
    fn a_square_picture_is_shown_whole() {
        assert_eq!(uv(256.0, 256.0), (0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn a_wide_picture_loses_its_sides_and_a_tall_one_its_top_and_bottom() {
        // 256x128: the middle half of the width.
        assert_eq!(uv(256.0, 128.0), (0.25, 0.0, 0.75, 1.0));
        // 128x256: the middle half of the height.
        assert_eq!(uv(128.0, 256.0), (0.0, 0.25, 1.0, 0.75));
    }

    #[test]
    fn a_picture_with_no_size_is_shown_as_it_is() {
        assert_eq!(uv(0.0, 100.0), (0.0, 0.0, 1.0, 1.0));
        assert_eq!(uv(100.0, 0.0), (0.0, 0.0, 1.0, 1.0));
    }
}
