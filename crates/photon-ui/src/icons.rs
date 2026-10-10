//! Icons from Lucide (https://lucide.dev), lucide-static 1.47.0. ISC License, Copyright (c)
//! Lucide Icons and Contributors. The licence is reproduced in THIRD-PARTY-NOTICES.md.
//!
//! The same path data as `ui/src/lib/icons.ts`, on Lucide's 24-unit grid, drawn the way
//! `Icon.svelte` draws it: a 2-unit round stroke, filled or not. Each is rasterised by
//! egui's SVG loader in white and tinted with a token where it is painted.

use eframe::egui::{self, Color32, Rect, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Bookmark,
    Calendar,
    ChevronDown,
    ChevronRight,
    Clock,
    Copy,
    EyeOff,
    Folder,
    Images,
    LayoutGrid,
    PanelLeft,
    Play,
    Settings,
    Star,
    Tag,
    TriangleAlert,
    User,
    X,
}

impl Icon {
    pub const ALL: [Icon; 18] = [
        Icon::Bookmark,
        Icon::Calendar,
        Icon::ChevronDown,
        Icon::ChevronRight,
        Icon::Clock,
        Icon::Copy,
        Icon::EyeOff,
        Icon::Folder,
        Icon::Images,
        Icon::LayoutGrid,
        Icon::PanelLeft,
        Icon::Play,
        Icon::Settings,
        Icon::Star,
        Icon::Tag,
        Icon::TriangleAlert,
        Icon::User,
        Icon::X,
    ];

    fn name(self) -> &'static str {
        match self {
            Icon::Bookmark => "bookmark",
            Icon::Calendar => "calendar",
            Icon::ChevronDown => "chevron-down",
            Icon::ChevronRight => "chevron-right",
            Icon::Folder => "folder",
            Icon::Images => "images",
            Icon::Tag => "tag",
            Icon::User => "user",
            Icon::Clock => "clock",
            Icon::Copy => "copy",
            Icon::EyeOff => "eye-off",
            Icon::LayoutGrid => "layout-grid",
            Icon::PanelLeft => "panel-left",
            Icon::Settings => "settings",
            Icon::X => "x",
            Icon::Play => "play",
            Icon::Star => "star",
            Icon::TriangleAlert => "triangle-alert",
        }
    }

    /// The inside of the icon's `<svg>`.
    fn inner(self) -> &'static str {
        match self {
            Icon::Calendar => {
                r#"<path d="M8 2v4"/><path d="M16 2v4"/><rect width="18" height="18" x="3" y="4" rx="2"/><path d="M3 10h18"/>"#
            }
            Icon::Clock => r#"<circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/>"#,
            Icon::Bookmark => r#"<path d="m19 21-7-4-7 4V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v16z"/>"#,
            Icon::ChevronDown => r#"<path d="m6 9 6 6 6-6"/>"#,
            Icon::ChevronRight => r#"<path d="m9 18 6-6-6-6"/>"#,
            Icon::Folder => {
                r#"<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>"#
            }
            Icon::Images => {
                r#"<path d="m22 11-1.296-1.296a2.4 2.4 0 0 0-3.408 0L11 16"/><path d="M4 8a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2"/><circle cx="13" cy="7" r="1" fill="currentColor"/><rect x="8" y="2" width="14" height="14" rx="2"/>"#
            }
            Icon::Tag => {
                r#"<path d="M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.42z"/><circle cx="7.5" cy="7.5" r=".5" fill="currentColor"/>"#
            }
            Icon::User => {
                r#"<path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/>"#
            }
            Icon::EyeOff => {
                r#"<path d="M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49"/><path d="M14.084 14.158a3 3 0 0 1-4.242-4.242"/><path d="M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143"/><path d="m2 2 20 20"/>"#
            }
            Icon::LayoutGrid => {
                r#"<rect width="7" height="7" x="3" y="3" rx="1"/><rect width="7" height="7" x="14" y="3" rx="1"/><rect width="7" height="7" x="14" y="14" rx="1"/><rect width="7" height="7" x="3" y="14" rx="1"/>"#
            }
            Icon::PanelLeft => {
                r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18"/>"#
            }
            Icon::Settings => {
                r#"<path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915"/><circle cx="12" cy="12" r="3"/>"#
            }
            Icon::X => r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#,
            Icon::Copy => {
                r#"<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>"#
            }
            Icon::Play => {
                r#"<path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"/>"#
            }
            Icon::Star => {
                r#"<path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z"/>"#
            }
            Icon::TriangleAlert => {
                r#"<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>"#
            }
        }
    }

    /// The whole document, in white: `filled` fills the outline as well as stroking it.
    /// `color` is white too, for the dots two icons fill with `currentColor`: left to its
    /// default that is black, which no tint lightens.
    pub fn svg(self, filled: bool) -> String {
        let fill = if filled { "white" } else { "none" };
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" color="white" fill="{fill}" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{}</svg>"#,
            self.inner()
        )
    }

    /// Paints the icon `size` points square, centred on `rect`, in `tint`.
    pub fn paint(self, ui: &egui::Ui, rect: Rect, size: f32, filled: bool, tint: Color32) {
        let uri = format!("bytes://photon-icon-{}-{filled}.svg", self.name());
        egui::Image::from_bytes(uri, self.svg(filled).into_bytes())
            .fit_to_exact_size(Vec2::splat(size))
            .tint(tint)
            .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(size)));
    }
}

/// The SVG loader the icons are drawn through. Once, at startup.
pub fn install(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    // The path data is a hand copy of `icons.ts`, which stays the source until the
    // switch-over: a Lucide update there must not leave this file a version behind.
    #[test]
    fn every_icon_is_the_svelte_uis() {
        let source = include_str!("../../../ui/src/lib/icons.ts");
        for icon in Icon::ALL {
            assert!(
                source.contains(&format!("'{}'", icon.inner())),
                "{} differs from icons.ts",
                icon.name()
            );
        }
    }

    // The dot of a tag and the sun of a picture are filled with `currentColor`, which is
    // black unless the document says otherwise, and the icons are tinted by multiplying.
    #[test]
    fn what_an_icon_fills_with_the_current_colour_is_white_like_the_rest() {
        assert!(Icon::Tag.inner().contains(r#"fill="currentColor""#));
        assert!(Icon::Tag.svg(false).contains(r#" color="white" "#));
    }

    #[test]
    fn an_icon_is_a_whole_document_filled_or_not() {
        let outline = Icon::Star.svg(false);
        assert!(outline.starts_with("<svg ") && outline.ends_with("</svg>"));
        assert!(outline.contains(r#"fill="none""#));
        assert!(Icon::Star.svg(true).contains(r#"fill="white""#));
    }
}
