//! The faces photon is set in: the platform's own interface face, as the Svelte UI's
//! `system-ui` was, and behind it an installed face for every script that one lacks.
//!
//! Found and registered by `fastframe-fonts` (MIT, pinned to a tag). Without its `inter`
//! feature nothing is bundled: where no platform face can be found or read, egui's own
//! fonts draw instead.

use eframe::egui::{self, FontId, Id};
use fastframe_fonts::{FontSetup, Primary, Weight};

/// Set in the context once `install` has registered the weights, so a context that never
/// had them - a test's - is not asked for a family it does not know, which panics.
fn marker() -> Id {
    Id::new("photon-fonts-installed")
}

/// Registers the faces. Once, at startup.
pub fn install(ctx: &egui::Context) {
    FontSetup::default()
        .primary(Primary::System)
        .weights(&[Weight::SemiBold])
        .install(ctx);
    ctx.data_mut(|data| data.insert_temp(marker(), true));
}

/// The regular weight at `size`.
pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

/// The semibold weight at `size`; the regular one in a context `install` has not seen.
pub fn semibold(ctx: &egui::Context, size: f32) -> FontId {
    let installed = ctx.data(|data| data.get_temp::<bool>(marker()).unwrap_or(false));
    if installed {
        Weight::SemiBold.font_id(size)
    } else {
        regular(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every headless test draws headers in a context without the installed faces.
    #[test]
    fn a_context_without_the_faces_is_given_the_regular_weight() {
        let ctx = egui::Context::default();
        assert_eq!(semibold(&ctx, 15.0), regular(15.0));
    }
}
