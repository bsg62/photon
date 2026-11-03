//! Shapes the window drew, kept and painted again: the viewer opens over a frozen picture of
//! the window, and a closed menu or dialog fades out from the layer it had. A view module
//! (it names egui types), so it is not in `STATE_MODULES`.

use crate::animate::eased;
use eframe::egui::{self, epaint::ClippedShape};

pub struct Afterimage {
    shapes: Vec<ClippedShape>,
    started_ms: f64,
    duration_ms: f64,
}

/// The shapes of the layers `keep` accepts, in paint order, as drawn so far in this frame
/// (egui empties them when the frame ends).
pub fn of_layers(
    ctx: &egui::Context,
    keep: impl Fn(egui::LayerId) -> bool,
    now_ms: f64,
    duration_ms: f64,
) -> Afterimage {
    // The ids first and the graphics after: both lock the context, so neither may run
    // inside the other's closure.
    let layers: Vec<egui::LayerId> = ctx.memory(|m| m.layer_ids().filter(|l| keep(*l)).collect());
    let shapes = ctx.graphics(|g| {
        layers
            .iter()
            .filter_map(|l| g.get(*l))
            .flat_map(|list| list.all_entries().cloned())
            .collect()
    });
    Afterimage {
        shapes,
        started_ms: now_ms,
        duration_ms,
    }
}

impl Afterimage {
    /// Paints every shape again at `opacity`, each within the clip rect it had. The painter
    /// applies the opacity to the shape itself, as it does to any shape it is given.
    pub fn paint(&self, painter: &egui::Painter, opacity: f32) {
        for kept in &self.shapes {
            let mut painter = painter.with_clip_rect(kept.clip_rect);
            painter.set_opacity(opacity);
            painter.add(kept.shape.clone());
        }
    }

    /// 1 at the start, 0 at the end, on the slide's curve.
    pub fn opacity(&self, now_ms: f64) -> f32 {
        1.0 - eased(now_ms - self.started_ms, self.duration_ms) as f32
    }

    pub fn running(&self, now_ms: f64) -> bool {
        now_ms - self.started_ms < self.duration_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{self, Color32, Id, LayerId, Order, Rect, Shape, pos2};

    fn rect_in(ctx: &egui::Context, name: &str, rect: Rect, color: Color32) {
        egui::Area::new(Id::new(name))
            .order(Order::Foreground)
            .fade_in(false)
            .fixed_pos(rect.min)
            .show(ctx, |ui| {
                // Clipped to the rect itself, so that a clip rect lost on the way shows.
                ui.painter_at(rect).rect_filled(rect, 0.0, color);
            });
    }

    /// A new area spends its first pass measuring itself, unseen: show them once before the
    /// pass a test reads.
    fn warm(ctx: &egui::Context, areas: &[(&str, Rect, Color32)]) {
        ctx.begin_pass(Default::default());
        for (name, rect, color) in areas {
            rect_in(ctx, name, *rect, *color);
        }
        let _ = end(ctx);
    }

    /// The pass's output, its texture deltas dropped: nothing here is drawn on a GPU.
    fn end(ctx: &egui::Context) -> egui::FullOutput {
        let mut output = ctx.end_pass();
        output.textures_delta.clear();
        output
    }

    fn fills(output: &egui::FullOutput) -> Vec<(Color32, Rect)> {
        output
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                Shape::Rect(r) if r.fill != Color32::TRANSPARENT => Some((r.fill, c.clip_rect)),
                _ => None,
            })
            .collect()
    }

    fn a() -> LayerId {
        LayerId::new(Order::Foreground, Id::new("a"))
    }

    #[test]
    fn an_afterimage_paints_what_its_layers_held_at_the_opacity_asked() {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_max(pos2(10.0, 10.0), pos2(50.0, 50.0));
        warm(&ctx, &[("a", rect, Color32::RED)]);
        ctx.begin_pass(Default::default());
        rect_in(&ctx, "a", rect, Color32::RED);
        let image = of_layers(&ctx, |l| l == a(), 0.0, 100.0);
        let first = end(&ctx);
        let clip = fills(&first)[0].1;
        assert_eq!(clip, rect);

        ctx.begin_pass(Default::default());
        image.paint(
            &ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new("again"))),
            0.5,
        );
        let second = end(&ctx);
        assert_eq!(
            fills(&second),
            vec![(Color32::RED.gamma_multiply(0.5), clip)]
        );
    }

    #[test]
    fn an_afterimage_leaves_out_the_layers_it_is_not_told_to_keep() {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_max(pos2(10.0, 10.0), pos2(50.0, 50.0));
        let other = rect.translate(egui::vec2(100.0, 0.0));
        warm(
            &ctx,
            &[("a", rect, Color32::RED), ("b", other, Color32::BLUE)],
        );
        ctx.begin_pass(Default::default());
        rect_in(&ctx, "a", rect, Color32::RED);
        rect_in(&ctx, "b", other, Color32::BLUE);
        let image = of_layers(&ctx, |l| l == a(), 0.0, 100.0);
        let _ = end(&ctx);

        ctx.begin_pass(Default::default());
        image.paint(
            &ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new("again"))),
            1.0,
        );
        let second = end(&ctx);
        let colours: Vec<Color32> = fills(&second).iter().map(|f| f.0).collect();
        assert_eq!(colours, vec![Color32::RED]);
    }

    #[test]
    fn an_afterimage_fades_over_its_time() {
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        let image = of_layers(&ctx, |_| true, 1000.0, 100.0);
        let _ = end(&ctx);
        assert_eq!(image.opacity(1000.0), 1.0);
        assert_eq!(image.opacity(1050.0), 1.0 - 0.875);
        assert!(image.running(1099.0));
        assert_eq!(image.opacity(1100.0), 0.0);
        assert!(!image.running(1100.0));
    }
}
