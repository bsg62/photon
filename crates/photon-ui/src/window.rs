//! The window photon opens: its size, its icon, and where what it last was is kept.
//!
//! **The window's own state is written by eframe and corrected by photon** (spec
//! `2026-10-16-photon-native-platform-design.md`, decision 1): its size, its place,
//! maximized and fullscreen, written as the window closes and every half minute into
//! `window.ron` beside `library.db`. eframe records the window as it is at that moment,
//! where the Tauri photon's window-state plugin records it only when it is in its normal
//! state: closed from the taskbar while minimized, a window on Windows is 0 by 0 at
//! -32000, and closed maximized, its normal size is the screen's, which it keeps when it
//! is next un-maximized. So photon follows the window's normal size and place in every
//! frame (`Seen`) and puts them into what eframe has just written (`App::save`, which
//! eframe calls after writing it), with the flags as the window has them. eframe keeps a
//! window on the monitors there are on Windows (`clamp_position_to_monitors`); a Mac keeps
//! a window on a screen itself; on X11 the window manager places a window whose monitor is
//! gone; on Wayland, where a window has no place it can ask for, the size alone is kept.
//! A slideshow quit in the middle reopens fullscreen, as the Tauri photon does, and F11
//! is the way out.

use crate::{args::Args, dirs::Dirs, dirs::IDENTIFIER};
use eframe::egui::{self, Pos2, Vec2};
use serde::{Deserialize, Serialize};

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
    let fullscreen = args.fullscreen;
    eframe::NativeOptions {
        viewport,
        // A run asked to be fullscreen - the gate's - keeps nothing, or the next launch
        // would open fullscreen. And it is fullscreen whatever was kept: eframe applies
        // what it kept over the builder (`with_fullscreen` among it), and applies this
        // hook after that.
        persist_window: !fullscreen,
        window_builder: fullscreen
            .then(|| Box::new(|builder: egui::ViewportBuilder| builder.with_fullscreen(true)) as _),
        persistence_path: Some(dirs.window_path.clone()),
        ..Default::default()
    }
}

/// The key eframe keeps the window under.
pub const KEY: &str = "window";

/// What eframe keeps of the window (`egui_winit::WindowSettings`), field for field and in
/// its serde shape, which keeps its fields to itself: read back, corrected and written
/// again in its place.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Kept {
    inner_position_pixels: Option<Pos2>,
    outer_position_pixels: Option<Pos2>,
    fullscreen: bool,
    maximized: bool,
    inner_size_points: Option<Vec2>,
}

/// The window's size and place in its normal state, neither minimized, maximized nor
/// fullscreen: what it comes back to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Normal {
    inner_position_pixels: Option<Pos2>,
    outer_position_pixels: Option<Pos2>,
    inner_size_points: Vec2,
}

/// What photon has seen of its window: its normal size and place, and the flags it last
/// had while it was not minimized.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Seen {
    normal: Option<Normal>,
    maximized: bool,
    fullscreen: bool,
    minimized: bool,
}

impl Seen {
    /// What was kept at the last launch, as the window comes back to it: its normal size
    /// and place, when it was kept in its normal state.
    pub fn launched(kept: Option<Kept>) -> Self {
        let normal = kept.and_then(|kept| {
            let normal = !kept.maximized && !kept.fullscreen;
            let size = kept
                .inner_size_points
                .filter(|size| normal && sized(*size))?;
            Some(Normal {
                inner_position_pixels: kept.inner_position_pixels,
                outer_position_pixels: kept.outer_position_pixels,
                inner_size_points: size,
            })
        });
        Self {
            normal,
            ..Self::default()
        }
    }

    /// The window as a frame finds it.
    pub fn frame(&mut self, info: &egui::ViewportInfo) {
        self.minimized = info.minimized == Some(true);
        if self.minimized {
            return;
        }
        self.maximized = info.maximized == Some(true);
        self.fullscreen = info.fullscreen == Some(true);
        if self.maximized || self.fullscreen {
            return;
        }
        let (Some(inner), Some(scale)) = (info.inner_rect, info.native_pixels_per_point) else {
            return;
        };
        if !sized(inner.size()) {
            return;
        }
        self.normal = Some(Normal {
            inner_position_pixels: Some(inner.min * scale),
            outer_position_pixels: info.outer_rect.map(|outer| outer.min * scale),
            inner_size_points: inner.size(),
        });
    }

    /// What eframe has just written, put right: the normal size and place wherever the
    /// window is not in its normal state, with the flags it had when it was last seen
    /// unminimized.
    pub fn correct(&self, written: Kept) -> Kept {
        if !self.minimized && !self.maximized && !self.fullscreen {
            return written;
        }
        let mut kept = written;
        if self.minimized {
            kept.maximized = self.maximized;
            kept.fullscreen = self.fullscreen;
        }
        if let Some(normal) = self.normal {
            kept.inner_position_pixels = normal.inner_position_pixels;
            kept.outer_position_pixels = normal.outer_position_pixels;
            kept.inner_size_points = Some(normal.inner_size_points);
        } else if self.minimized {
            // Nothing seen to put in place of a minimized window's 0 by 0: none at all,
            // which leaves the next launch the window's default size.
            kept.inner_position_pixels = None;
            kept.outer_position_pixels = None;
            kept.inner_size_points = None;
        }
        kept
    }
}

/// Puts right what eframe has just written of the window in `storage`, from what `seen`
/// saw of it; with nothing seen - a run that keeps nothing of its window - what was kept
/// before is left as it was.
pub fn save(seen: Option<&Seen>, storage: &mut dyn eframe::Storage) {
    if let Some(seen) = seen
        && let Some(written) = eframe::get_value::<Kept>(storage, KEY)
    {
        eframe::set_value(storage, KEY, &seen.correct(written));
    }
}

/// Whether a size is one a window can be opened at.
fn sized(size: Vec2) -> bool {
    size.x >= 1.0 && size.y >= 1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, path::Path};

    /// eframe's storage, in memory.
    #[derive(Default)]
    struct Memory(HashMap<String, String>);

    impl eframe::Storage for Memory {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.to_owned(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }

    /// A window seen in its normal state at (100, 50), 900 by 600, at two pixels a point;
    /// then as `maximized`, `fullscreen` and `minimized` say.
    fn seen(maximized: bool, fullscreen: bool, minimized: bool) -> Seen {
        let mut seen = Seen::default();
        let info = |maximized, fullscreen, minimized, size: Vec2| egui::ViewportInfo {
            native_pixels_per_point: Some(2.0),
            inner_rect: Some(egui::Rect::from_min_size(egui::pos2(100.0, 80.0), size)),
            outer_rect: Some(egui::Rect::from_min_size(egui::pos2(100.0, 50.0), size)),
            maximized: Some(maximized),
            fullscreen: Some(fullscreen),
            minimized: Some(minimized),
            ..Default::default()
        };
        seen.frame(&info(false, false, false, egui::vec2(900.0, 600.0)));
        // What a window not in its normal state says of itself: the screen's size, or
        // nothing at all - and none of it is taken for the normal one.
        let size = if minimized {
            Vec2::ZERO
        } else {
            egui::vec2(1920.0, 1080.0)
        };
        seen.frame(&info(maximized, fullscreen, minimized, size));
        seen
    }

    /// What eframe writes of a window as it is: the screen's size, or 0 by 0 at -32000.
    fn written(maximized: bool, fullscreen: bool, minimized: bool) -> Kept {
        let (place, size) = if minimized {
            (egui::pos2(-32000.0, -32000.0), Vec2::ZERO)
        } else {
            (Pos2::ZERO, egui::vec2(1920.0, 1080.0))
        };
        Kept {
            inner_position_pixels: Some(place),
            outer_position_pixels: Some(place),
            fullscreen,
            maximized,
            inner_size_points: Some(size),
        }
    }

    fn normal(maximized: bool, fullscreen: bool) -> Kept {
        Kept {
            inner_position_pixels: Some(egui::pos2(200.0, 160.0)),
            outer_position_pixels: Some(egui::pos2(200.0, 100.0)),
            fullscreen,
            maximized,
            inner_size_points: Some(egui::vec2(900.0, 600.0)),
        }
    }

    // What photon writes in eframe's place is eframe's own record, field for field: read
    // by eframe as its `WindowSettings` and written back, it is what it was.
    #[test]
    fn what_is_kept_has_eframes_shape() {
        let mut memory = Memory::default();
        let kept = normal(true, false);
        eframe::set_value(&mut memory, KEY, &kept);
        let theirs: egui_winit::WindowSettings = eframe::get_value(&memory, KEY).unwrap();
        assert_eq!(theirs.inner_size_points(), Some(egui::vec2(900.0, 600.0)));
        eframe::set_value(&mut memory, KEY, &theirs);
        assert_eq!(eframe::get_value::<Kept>(&memory, KEY), Some(kept));
    }

    // Closed from the taskbar while minimized, a window is kept as it was before it was
    // minimized - maximized, here - and at its normal size and place, not at 0 by 0.
    #[test]
    fn a_window_closed_minimized_is_kept_as_it_was_before() {
        let mut was = seen(true, false, false);
        was.frame(&egui::ViewportInfo {
            minimized: Some(true),
            inner_rect: Some(egui::Rect::ZERO),
            native_pixels_per_point: Some(2.0),
            ..Default::default()
        });
        assert_eq!(
            was.correct(written(false, false, true)),
            normal(true, false)
        );
        assert_eq!(
            seen(false, false, true).correct(written(false, false, true)),
            normal(false, false)
        );
    }

    // Closed maximized or fullscreen, a window keeps its normal size and place under the
    // flag, so that it comes back to them when it is next made normal.
    #[test]
    fn a_window_closed_maximized_keeps_its_normal_size() {
        assert_eq!(
            seen(true, false, false).correct(written(true, false, false)),
            normal(true, false)
        );
        assert_eq!(
            seen(false, true, false).correct(written(false, true, false)),
            normal(false, true)
        );
    }

    #[test]
    fn a_window_in_its_normal_state_is_kept_as_eframe_wrote_it() {
        let normal_now = seen(false, false, false);
        assert_eq!(
            normal_now.correct(written(false, false, false)),
            written(false, false, false)
        );
    }

    // A window minimized with nothing seen of its normal state is kept at no size, which
    // leaves the next launch the default, not at 0 by 0.
    #[test]
    fn a_window_never_seen_normal_is_kept_at_no_size() {
        let mut never = Seen::launched(Some(written(true, false, false)));
        never.frame(&egui::ViewportInfo {
            minimized: Some(true),
            ..Default::default()
        });
        let kept = never.correct(written(false, false, true));
        assert_eq!(kept.inner_size_points, None);
        assert_eq!(kept.outer_position_pixels, None);
    }

    // Launched from a window kept in its normal state, that is its normal size until it
    // is seen; kept maximized, the screen's size is no normal one.
    #[test]
    fn the_normal_size_kept_is_the_one_launched_with() {
        let launched = Seen::launched(Some(normal(false, false)));
        let mut was = launched;
        was.frame(&egui::ViewportInfo {
            minimized: Some(true),
            ..Default::default()
        });
        assert_eq!(
            was.correct(written(false, false, true)),
            normal(false, false)
        );
        assert_eq!(
            Seen::launched(Some(written(true, false, false))).normal,
            None
        );
    }

    // A window that says it has no size - as one not yet shown on screen can - is no
    // normal one to come back to.
    #[test]
    fn a_window_of_no_size_is_no_normal_one() {
        let mut seen = seen(true, false, false);
        seen.frame(&egui::ViewportInfo {
            native_pixels_per_point: Some(2.0),
            inner_rect: Some(egui::Rect::ZERO),
            maximized: Some(false),
            fullscreen: Some(false),
            minimized: Some(false),
            ..Default::default()
        });
        seen.frame(&egui::ViewportInfo {
            maximized: Some(true),
            ..Default::default()
        });
        assert_eq!(
            seen.correct(written(true, false, false)),
            normal(true, false)
        );
        let mut no_size = normal(false, false);
        no_size.inner_size_points = Some(Vec2::ZERO);
        assert_eq!(Seen::launched(Some(no_size)).normal, None);
    }

    // A run that keeps nothing of its window leaves what was kept as it was.
    #[test]
    fn a_run_that_keeps_nothing_corrects_nothing() {
        let mut memory = Memory::default();
        eframe::set_value(&mut memory, KEY, &written(false, false, true));
        save(None, &mut memory);
        assert_eq!(
            eframe::get_value::<Kept>(&memory, KEY),
            Some(written(false, false, true))
        );
        save(Some(&seen(false, false, true)), &mut memory);
        assert_eq!(
            eframe::get_value::<Kept>(&memory, KEY),
            Some(normal(false, false))
        );
    }

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
        // And fullscreen over what was kept, which eframe puts on the builder first.
        let kept = egui::ViewportBuilder::default().with_fullscreen(false);
        let hook = options.window_builder.expect("the hook");
        assert_eq!(hook(kept).fullscreen, Some(true));
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
