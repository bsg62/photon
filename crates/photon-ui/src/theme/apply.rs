//! The tokens onto egui: its visuals for each theme, and which theme is in force.

use super::tokens::{DARK, LIGHT, Palette, Rgba, SHADOW_DIALOG, SHADOW_MENU};
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

/// `--shadow-dialog`, as egui draws a shadow.
pub fn dialog_shadow() -> Shadow {
    Shadow {
        offset: [SHADOW_DIALOG.x as i8, SHADOW_DIALOG.y as i8],
        blur: SHADOW_DIALOG.blur as u8,
        spread: 0,
        color: color(SHADOW_DIALOG.ink),
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
        // A text field's placeholder: egui writes one in this colour whatever colour
        // the words were given, and left to itself greys out the text's.
        visuals.weak_text_color = Some(color(palette.text_dim));
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

/// The user's choice put on the window: egui's visuals, and the title bar the system
/// draws, which is the window's own and not egui's - pinned against the desktop's, a dark
/// photon stood under a light title bar. `System` is no theme of the window's own, which
/// is what lets the bar go on following the desktop.
pub fn put(ctx: &egui::Context, choice: ThemeChoice) {
    choose(ctx, choice);
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(match choice {
        ThemeChoice::System => egui::SystemTheme::SystemDefault,
        ThemeChoice::Light => egui::SystemTheme::Light,
        ThemeChoice::Dark => egui::SystemTheme::Dark,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The theme the window was told in a frame that puts `choice`.
    fn told(choice: ThemeChoice) -> (Option<egui::SystemTheme>, ThemePreference) {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| put(ui.ctx(), choice));
        output.textures_delta.clear();
        let told = (output.viewport_output.get(&egui::ViewportId::ROOT)).and_then(|viewport| {
            viewport.commands.iter().find_map(|command| match command {
                egui::ViewportCommand::SetTheme(theme) => Some(*theme),
                _ => None,
            })
        });
        (told, ctx.options(|options| options.theme_preference))
    }

    // egui's visuals and the window's own title bar, each told the choice; `System` is
    // the desktop's, for both.
    #[test]
    fn a_theme_put_is_put_on_the_title_bar_too() {
        assert_eq!(
            told(ThemeChoice::Dark),
            (Some(egui::SystemTheme::Dark), ThemePreference::Dark)
        );
        assert_eq!(
            told(ThemeChoice::Light),
            (Some(egui::SystemTheme::Light), ThemePreference::Light)
        );
        assert_eq!(
            told(ThemeChoice::System),
            (
                Some(egui::SystemTheme::SystemDefault),
                ThemePreference::System
            )
        );
    }

    // egui paints a field's placeholder in its "weak" text colour and in no other,
    // whatever the words handed to it say: every field here gives its placeholder the
    // palette's dim text, and until this was set none of them was drawn in it.
    #[test]
    fn a_placeholder_is_written_in_the_dim_text_of_its_theme() {
        let ctx = egui::Context::default();
        install(&ctx);
        for (theme, palette) in [(Theme::Light, &LIGHT), (Theme::Dark, &DARK)] {
            assert_eq!(
                ctx.style_of(theme).visuals.weak_text_color(),
                color(palette.text_dim),
                "{theme:?}"
            );
        }
    }
}
