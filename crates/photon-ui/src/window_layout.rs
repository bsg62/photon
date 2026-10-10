//! How the window is laid out: its four areas around the grid, and the three things the
//! user sets about them - the sidebar's width, whether it is hidden, which of its groups
//! are open.
//!
//! Those three are kept per machine, in `layout.json` beside `library.db`, and not in the
//! library's settings table: they are how this window is laid out here, not a fact about
//! the library, and that table's accessors are photon-core's, which the Tauri photon reads
//! too. The Svelte UI kept them in the web view's `localStorage` (`lib/sidebar.ts`), which
//! is where the numbers and the rules are from.
//!
//! No egui here: an area is four numbers.

use std::path::Path;

pub const SIDEBAR_DEFAULT: f32 = 260.0;
pub const SIDEBAR_MIN: f32 = 160.0;
/// What an arrow key moves the focused splitter by.
pub const SIDEBAR_STEP: f32 = 16.0;
pub const SPLITTER: f32 = 5.0;
/// The top bar: the search field's thirty points, eight above and below, and its line.
pub const TOP_BAR: f32 = 46.0;
pub const STATUS_BAR: f32 = 23.0;

/// Holds a width to between `SIDEBAR_MIN` and half the window. The minimum wins when the
/// window is too narrow for both, so the list never collapses to nothing.
pub fn clamp_sidebar_width(width: f32, window: f32) -> f32 {
    width.min(window / 2.0).max(SIDEBAR_MIN).round()
}

/// Which of the sidebar's groups are unfolded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenGroups {
    pub albums: bool,
    pub searches: bool,
    pub people: bool,
    pub tags: bool,
}

/// Albums and searches start open because they are the user's own; People and Tags start
/// closed because a real library has hundreds of each, and the years below must stay
/// reachable.
impl Default for OpenGroups {
    fn default() -> Self {
        Self {
            albums: true,
            searches: true,
            people: false,
            tags: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// The width last dragged to. It may have been set in a wider window, so it is
    /// clamped where it is used (`shown_width`), never where it is read.
    pub sidebar_width: f32,
    pub sidebar_hidden: bool,
    pub open: OpenGroups,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            sidebar_width: SIDEBAR_DEFAULT,
            sidebar_hidden: false,
            open: OpenGroups::default(),
        }
    }
}

impl Layout {
    /// The layout `text` holds, each thing that is not there, or not what it should be, at
    /// its default: a file from another version, or one cut short, must not cost the user
    /// the parts of it that can be read.
    pub fn read(text: &str) -> Self {
        let mut layout = Self::default();
        let Ok(stored) = serde_json::from_str::<serde_json::Value>(text) else {
            return layout;
        };
        if let Some(width) = stored["sidebarWidth"].as_f64()
            && width.is_finite()
            && width > 0.0
        {
            layout.sidebar_width = width as f32;
        }
        // Only a stored yes hides it: hidden, the way back is one small button and a key,
        // so anything else leaves it where a new user expects it.
        layout.sidebar_hidden = stored["sidebarHidden"] == true;
        let open = &stored["openGroups"];
        for (name, group) in [
            ("albums", &mut layout.open.albums),
            ("searches", &mut layout.open.searches),
            ("people", &mut layout.open.people),
            ("tags", &mut layout.open.tags),
        ] {
            if let Some(stored) = open[name].as_bool() {
                *group = stored;
            }
        }
        layout
    }

    pub fn written(&self) -> String {
        serde_json::json!({
            "sidebarWidth": self.sidebar_width,
            "sidebarHidden": self.sidebar_hidden,
            "openGroups": {
                "albums": self.open.albums,
                "searches": self.open.searches,
                "people": self.open.people,
                "tags": self.open.tags,
            },
        })
        .to_string()
    }

    /// The layout stored at `path`, or the defaults when there is none to be read.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).map_or_else(|_| Self::default(), |text| Self::read(&text))
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, self.written())
    }

    /// The width the sidebar is drawn at in a window `window` wide.
    pub fn shown_width(&self, window: f32) -> f32 {
        clamp_sidebar_width(self.sidebar_width, window)
    }
}

/// A rectangle of the window, in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Area {
    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }
}

/// The window's four areas. The sidebar and its splitter are not there when it is hidden:
/// the content then begins at the window's left edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Areas {
    pub top_bar: Area,
    pub sidebar: Option<Area>,
    pub splitter: Option<Area>,
    pub content: Area,
    pub status_bar: Area,
}

pub fn areas(width: f32, height: f32, layout: &Layout) -> Areas {
    let across = |top: f32, bottom: f32| Area {
        left: 0.0,
        top,
        right: width,
        bottom,
    };
    let top = TOP_BAR.min(height);
    let bottom = (height - STATUS_BAR).max(top);
    let side = if layout.sidebar_hidden {
        0.0
    } else {
        layout.shown_width(width)
    };
    let column = |left: f32, right: f32| Area {
        left,
        top,
        right,
        bottom,
    };
    let shown = !layout.sidebar_hidden;
    Areas {
        top_bar: across(0.0, top),
        sidebar: shown.then(|| column(0.0, side)),
        splitter: shown.then(|| column(side, side + SPLITTER)),
        content: column(if shown { side + SPLITTER } else { 0.0 }, width),
        status_bar: across(bottom, height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_width_is_held_between_the_minimum_and_half_the_window() {
        assert_eq!(clamp_sidebar_width(260.0, 1280.0), 260.0);
        assert_eq!(clamp_sidebar_width(900.0, 1280.0), 640.0);
        assert_eq!(clamp_sidebar_width(40.0, 1280.0), SIDEBAR_MIN);
        assert_eq!(clamp_sidebar_width(200.4, 1280.0), 200.0);
        // Too narrow for both: the minimum wins, so the list never collapses to nothing.
        assert_eq!(clamp_sidebar_width(260.0, 300.0), SIDEBAR_MIN);
    }

    #[test]
    fn a_layout_is_read_back_as_it_was_written() {
        let layout = Layout {
            sidebar_width: 312.0,
            sidebar_hidden: true,
            open: OpenGroups {
                albums: false,
                searches: true,
                people: true,
                tags: false,
            },
        };
        assert_eq!(Layout::read(&layout.written()), layout);
        assert_eq!(
            Layout::read(&Layout::default().written()),
            Layout::default()
        );
    }

    // A file from another version, a file cut short, a file someone edited: whatever of it
    // can be read is kept, and the rest is as a new user finds it.
    #[test]
    fn what_cannot_be_read_is_at_its_default() {
        assert_eq!(Layout::read(""), Layout::default());
        assert_eq!(Layout::read("{\"sidebarWidth\": 3"), Layout::default());
        assert_eq!(Layout::read("[1, 2]"), Layout::default());

        let partly = Layout::read(
            r#"{"sidebarWidth": "wide", "sidebarHidden": "yes",
                "openGroups": {"albums": false, "people": 1, "tags": true}}"#,
        );
        assert_eq!(partly.sidebar_width, SIDEBAR_DEFAULT);
        assert!(!partly.sidebar_hidden, "only a stored yes hides it");
        assert_eq!(
            partly.open,
            OpenGroups {
                albums: false,
                searches: true,
                people: false,
                tags: true,
            }
        );
        // A width nobody chose.
        assert_eq!(
            Layout::read(r#"{"sidebarWidth": 0}"#).sidebar_width,
            SIDEBAR_DEFAULT
        );
        assert_eq!(
            Layout::read(r#"{"sidebarWidth": -40}"#).sidebar_width,
            SIDEBAR_DEFAULT
        );
    }

    #[test]
    fn a_layout_that_is_not_stored_is_the_defaults_and_one_saved_is_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.json");
        assert_eq!(Layout::load(&path), Layout::default());
        let layout = Layout {
            sidebar_width: 200.0,
            ..Layout::default()
        };
        layout.save(&path).unwrap();
        assert_eq!(Layout::load(&path), layout);
    }

    // The width is stored as it was dragged, and clamped where it is shown: stored clamped,
    // a window narrowed for a moment would cost the user the width they chose.
    #[test]
    fn a_stored_width_is_clamped_to_the_window_it_is_shown_in() {
        let layout = Layout {
            sidebar_width: 500.0,
            ..Layout::default()
        };
        assert_eq!(layout.shown_width(1600.0), 500.0);
        assert_eq!(layout.shown_width(800.0), 400.0);
        assert_eq!(layout.sidebar_width, 500.0);
    }

    #[test]
    fn the_four_areas_fill_the_window() {
        let found = areas(1280.0, 800.0, &Layout::default());
        assert_eq!(
            found.top_bar,
            Area {
                left: 0.0,
                top: 0.0,
                right: 1280.0,
                bottom: TOP_BAR
            }
        );
        let sidebar = found.sidebar.unwrap();
        assert_eq!((sidebar.left, sidebar.right), (0.0, 260.0));
        assert_eq!((sidebar.top, sidebar.bottom), (TOP_BAR, 800.0 - STATUS_BAR));
        let splitter = found.splitter.unwrap();
        assert_eq!((splitter.left, splitter.right), (260.0, 265.0));
        assert_eq!(found.content.left, 265.0);
        assert_eq!(found.content.right, 1280.0);
        assert_eq!(found.content.height(), 800.0 - TOP_BAR - STATUS_BAR);
        assert_eq!(found.status_bar.top, 800.0 - STATUS_BAR);
        assert_eq!(found.status_bar.height(), STATUS_BAR);
    }

    #[test]
    fn a_hidden_sidebar_gives_its_room_to_the_content() {
        let hidden = Layout {
            sidebar_hidden: true,
            ..Layout::default()
        };
        let found = areas(1280.0, 800.0, &hidden);
        assert_eq!(found.sidebar, None);
        assert_eq!(found.splitter, None);
        assert_eq!(found.content.left, 0.0);
        assert_eq!(found.content.width(), 1280.0);
    }

    // A window being dragged very small must not hand out an area that ends before it
    // begins.
    #[test]
    fn no_area_is_inside_out_in_a_window_too_small_for_the_bars() {
        for (width, height) in [(200.0, 40.0), (0.0, 0.0), (120.0, 60.0)] {
            let found = areas(width, height, &Layout::default());
            for area in [found.top_bar, found.content, found.status_bar] {
                assert!(area.height() >= 0.0, "{area:?} in {width}x{height}");
            }
        }
    }
}
