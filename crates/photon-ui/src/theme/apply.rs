//! The tokens onto egui: its visuals for each theme, and which theme is in force.

use super::tokens::{DARK, LIGHT, Palette, Rgba, SHADOW_MENU};
use eframe::egui::{self, Color32, Theme, ThemePreference, epaint::Shadow};
use photon_core::library::ThemeChoice;

pub fn color(Rgba(r, g, b, a): Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// `--shadow-menu`, as egui draws a shadow: under whatever lies over the window.
pub fn menu_shadow() -> Shadow {
    Shadow {
        offset: [SHADOW_MENU.x as i8, SHADOW_MENU.y as i8],
        blur: SHADOW_MENU.blur as u8,
        spread: 0,
        color: color(SHADOW_MENU.ink),
    }
}

/// The palette of the theme egui is drawing in.
pub fn palette(ctx: &egui::Context) -> &'static Palette {
    match ctx.theme() {
        Theme::Dark => &DARK,
        Theme::Light => &LIGHT,
    }
}

/// Sets both themes' visuals from the tokens, once, at startup.
pub fn install(ctx: &egui::Context) {
    for (theme, palette) in [(Theme::Light, &LIGHT), (Theme::Dark, &DARK)] {
        let mut visuals = match theme {
            Theme::Dark => egui::Visuals::dark(),
            Theme::Light => egui::Visuals::light(),
        };
        visuals.panel_fill = color(palette.surface);
        visuals.window_fill = color(palette.raised);
        visuals.extreme_bg_color = color(palette.surface);
        visuals.faint_bg_color = color(palette.hover);
        visuals.override_text_color = Some(color(palette.text));
        visuals.hyperlink_color = color(palette.accent);
        visuals.selection.bg_fill = color(palette.accent_soft);
        visuals.selection.stroke.color = color(palette.accent);
        ctx.set_visuals_of(theme, visuals);
    }
}

/// The user's choice. `System` is egui's own preference of that name and not the scheme
/// resolved here, which is what lets the window keep following the desktop while photon
/// runs - the reason `app.rs` gives for the Tauri title bar.
pub fn choose(ctx: &egui::Context, choice: ThemeChoice) {
    ctx.set_theme(match choice {
        ThemeChoice::System => ThemePreference::System,
        ThemeChoice::Light => ThemePreference::Light,
        ThemeChoice::Dark => ThemePreference::Dark,
    });
}
