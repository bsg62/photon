//! The window photon opens: its size, its icon, and where what it last was is kept.
//!
//! **The window's own state is eframe's to keep** (spec
//! `2026-10-16-photon-native-platform-design.md`, decision 1): its size, its place,
//! maximized and fullscreen, written by eframe as the window closes and every half minute,
//! into `window.ron` beside `library.db`. eframe puts a window back only where a monitor
//! still is (`clamp_position_to_monitors`) and, on Wayland, where a window has no place it
//! can ask for, keeps its size alone - what the Tauri photon's window-state plugin does, and
//! what photon would otherwise rebuild. A slideshow quit in the middle reopens fullscreen,
//! as the Tauri photon does, and F11 is the way out.

use crate::{args::Args, dirs::Dirs, dirs::IDENTIFIER};
use eframe::egui;

/// photon's icon, the installers' own (256 pixels a side), for the title bar and the
/// taskbar.
const ICON_PNG: &[u8] = include_bytes!("../icons/photon-256.png");

/// The window's options for this run.
pub fn native_options(args: &Args, dirs: &Dirs) -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("photon")
        // What a Wayland compositor and a taskbar know the window by.
        .with_app_id(IDENTIFIER)
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([800.0, 500.0])
        .with_fullscreen(args.fullscreen);
    match eframe::icon_data::from_png_bytes(ICON_PNG) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        // The window opens with the system's icon: no reason to open none.
        Err(err) => tracing::warn!(%err, "photon's icon could not be read"),
    }
    eframe::NativeOptions {
        viewport,
        // A run asked to be fullscreen - the gate's - keeps nothing and is given nothing:
        // what eframe kept overrides the fullscreen asked for, and a gate run would leave
        // the next launch fullscreen.
        persist_window: !args.fullscreen,
        persistence_path: Some(dirs.window_path.clone()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn dirs() -> Dirs {
        crate::dirs::within(Path::new("/data/app"), Path::new("/cache/app"))
    }

    // The window is kept beside the library, in a file of photon's own - not in eframe's
    // own place for an application's data, which would be a second home for one library.
    #[test]
    fn the_window_is_kept_beside_the_library() {
        let options = native_options(&Args::default(), &dirs());
        assert!(options.persist_window);
        assert_eq!(
            options.persistence_path.as_deref(),
            Some(Path::new("/data/app/window.ron"))
        );
    }

    #[test]
    fn a_run_asked_to_be_fullscreen_keeps_no_window() {
        let args = Args {
            fullscreen: true,
            ..Args::default()
        };
        let options = native_options(&args, &dirs());
        assert!(!options.persist_window);
        assert_eq!(options.viewport.fullscreen, Some(true));
    }

    #[test]
    fn the_window_has_photons_icon() {
        let options = native_options(&Args::default(), &dirs());
        let icon = options.viewport.icon.expect("an icon");
        assert_eq!((icon.width, icon.height), (256, 256));
        // The installers' icon, not a stand-in: the same bytes as the Tauri bundle's.
        assert_eq!(
            ICON_PNG,
            include_bytes!("../../photon-app/icons/128x128@2x.png").as_slice()
        );
    }
}
