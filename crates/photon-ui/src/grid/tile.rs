//! One tile: a square, the photo covering it, and its marks.

use super::labels::format_duration;
use crate::{
    icons::Icon,
    theme::{
        apply::color,
        fonts,
        tokens::{PHOTO_LINE, Palette, R, SHADOW_INK, T},
    },
};
use eframe::egui::{
    self, Align2, Color32, Pos2, Rect, Stroke, StrokeKind, TextureHandle, Vec2, pos2, vec2,
};
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

/// How wide the ring of a selected tile is, and the line that sets it off the photo.
const RING: f32 = 2.0;
const RING_EDGE: f32 = 1.0;

/// How much of a tile is seen whose folder is offline: the Svelte tile's `.dimmed`.
const DIMMED: f32 = 0.4;

/// Paints `entry`'s tile in `rect`. `texture` is its thumbnail when it has arrived, with
/// how much of it is seen (less while it fades in),
/// `failed` whether it never will, `selected` whether it is one of the photos picked, and
/// `dimmed` whether its folder is offline - the whole tile at `DIMMED`, as the Svelte
/// tile's opacity takes its marks and its ring with it.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    ui: &mut egui::Ui,
    rect: Rect,
    entry: &GridEntry,
    texture: Option<(&TextureHandle, f32)>,
    failed: bool,
    selected: bool,
    dimmed: bool,
    palette: &Palette,
) {
    if !dimmed {
        return paint_tile(ui, rect, entry, texture, failed, selected, palette);
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.multiply_opacity(DIMMED);
        paint_tile(ui, rect, entry, texture, failed, selected, palette);
    });
}

fn paint_tile(
    ui: &egui::Ui,
    rect: Rect,
    entry: &GridEntry,
    texture: Option<(&TextureHandle, f32)>,
    failed: bool,
    selected: bool,
    palette: &Palette,
) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, R[1], color(palette.field));
    if let Some((texture, shown)) = texture {
        let size = texture.size_vec2();
        egui::Image::from_texture(texture)
            .uv(cover_uv(size.x, size.y))
            .tint(Color32::WHITE.gamma_multiply(shown))
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
        halo(on_photo, |shift, tint| {
            Icon::Play.paint(ui, play.translate(shift), PLAY, true, tint);
        });
        if let Some(ms) = entry.duration_ms {
            let words = format_duration(ms);
            halo(on_photo, |shift, tint| {
                painter.text(
                    pos2(play.right() + 3.0, play.center().y) + shift,
                    Align2::LEFT_CENTER,
                    &words,
                    fonts::regular(T[0]),
                    tint,
                );
            });
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
    if selected {
        // Inside the tile, not around it: a row lies flush to the top of the grid, and
        // anything outside the tile's own square is cut there. The thin line inside the
        // ring, in the ground's colour, keeps it legible over a photo of the accent's
        // own blue.
        painter.rect_stroke(
            rect,
            R[1],
            Stroke::new(RING, color(palette.accent)),
            StrokeKind::Inside,
        );
        painter.rect_stroke(
            rect.shrink(RING),
            (R[1] - RING).max(0.0),
            Stroke::new(RING_EDGE, color(palette.surface)),
            StrokeKind::Inside,
        );
    }
}

fn mark(ui: &egui::Ui, icon: Icon, corner: Pos2, filled: bool, tint: Color32) {
    let place = Rect::from_min_size(corner, vec2(MARK, MARK));
    halo(tint, |shift, tint| {
        icon.paint(ui, place.translate(shift), MARK, filled, tint);
    });
}

/// Something drawn on the photo, with a dark halo round it: drawn in the shadow's ink a
/// point out each way, then itself. The Svelte tile's `drop-shadow(0 0 2px
/// var(--shadow-ink))` - without it a white mark, the copies mark or a video's length,
/// was lost on a bright photo, and the star on an amber one.
fn halo(tint: Color32, mut paint: impl FnMut(Vec2, Color32)) {
    let shadow = color(SHADOW_INK).gamma_multiply(0.5);
    for (x, y) in [
        (-1.0, -1.0),
        (0.0, -1.0),
        (1.0, -1.0),
        (-1.0, 0.0),
        (1.0, 0.0),
        (-1.0, 1.0),
        (0.0, 1.0),
        (1.0, 1.0),
    ] {
        paint(vec2(x, y), shadow);
    }
    paint(Vec2::ZERO, tint);
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

    // A video's length is drawn over a halo of the shadow's ink, a point out each way, as
    // every mark on the photo is: alone, white on a bright photo, it was not there.
    #[test]
    fn a_mark_on_the_photo_has_a_halo() {
        let ctx = egui::Context::default();
        let entry = GridEntry {
            id: 1,
            folder_id: 1,
            taken_at: 0,
            aspect: 1.0,
            kind: MediaKind::Video,
            duration_ms: Some(5_000),
            starred: false,
            has_copies: false,
            thumb_key: 0,
            size: 0,
            mtime_ms: 0,
        };
        let mut full = ctx.run_ui(Default::default(), |ui| {
            let rect = Rect::from_min_size(pos2(10.0, 10.0), vec2(120.0, 120.0));
            paint(
                ui,
                rect,
                &entry,
                None,
                false,
                false,
                false,
                &crate::theme::tokens::LIGHT,
            );
        });
        full.textures_delta.clear();
        let lengths: Vec<Color32> = (full.shapes.iter())
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == format_duration(5_000) => {
                    Some(text.fallback_color)
                }
                _ => None,
            })
            .collect();
        let shadow = color(SHADOW_INK).gamma_multiply(0.5);
        assert_eq!(lengths.len(), 9, "the length and its halo");
        assert_eq!(lengths.iter().filter(|tint| **tint == shadow).count(), 8);
        assert_eq!(
            lengths.last(),
            Some(&color(PHOTO_LINE)),
            "the length over it"
        );
    }
}
