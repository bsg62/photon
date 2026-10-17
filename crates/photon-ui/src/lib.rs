//! photon-ui: photon's interface drawn by egui on the GPU, over photon-engine.
//!
//! Two kinds of module. *State modules* are plain Rust that names no egui type and is
//! tested as such: geometry, motion, the scroll position, tasks, the thumbnail loader and
//! its bookkeeping, the tokens, the labels. *Views* draw a state module and turn input
//! into calls on it. `state_modules_name_no_egui_type` holds the line.

pub mod allocator;
pub mod animate;
pub mod app;
pub mod args;
pub mod compare;
pub mod compare_view;
pub mod controls;
pub mod controls_bar;
pub mod copies;
pub mod crop;
pub mod crop_tool;
pub mod crop_view;
pub mod dialog_view;
pub mod dialogs;
pub mod dirs;
pub mod drop_card;
pub mod empty;
pub mod empty_panel;
pub mod events;
pub mod export_dialog;
pub mod export_view;
pub mod face_crops;
pub mod fixture;
pub mod folder_drop;
pub mod gpu;
pub mod grid {
    pub mod glide;
    pub mod header;
    pub mod labels;
    pub mod layout;
    pub mod motion;
    pub mod scroll;
    pub mod tile;
    pub mod timeline;
    pub mod view;
    pub mod visible;
}
pub mod icons;
pub mod locale;
pub mod menu;
pub mod menu_view;
pub mod nav;
pub mod people_page;
pub mod people_view;
pub mod person_picker;
pub mod platform;
pub mod playback;
pub mod posters;
pub mod probe;
pub mod rename;
pub mod scans;
pub mod scroll_bar;
pub mod search_bar;
pub mod search_box;
pub mod search_help;
pub mod select;
pub mod select_view;
pub mod selection;
pub mod settings;
pub mod settings_view;
pub mod shell;
pub mod shortcuts;
pub mod shortcuts_view;
pub mod side_menu;
pub mod slideshow;
pub mod slideshow_order;
pub mod stats;
pub mod status;
pub mod sidebar {
    pub mod folders;
    pub mod list;
    pub mod rows;
    pub mod view;
}
pub mod tag_manager;
pub mod tag_picker;
pub mod tasks;
pub mod text;
pub mod theme {
    pub mod apply;
    pub mod fonts;
    pub mod tokens;
}
pub mod tile_menu;
pub mod thumbs {
    pub mod loader;
    pub mod shown;
    pub mod source;
    pub mod textures;
}
pub mod toasts;
pub mod video;
pub mod viewer;
pub mod viewer_caption;
pub mod viewer_faces;
pub mod viewer_histogram;
pub mod viewer_info;
pub mod viewer_info_view;
pub mod viewer_layout;
pub mod viewer_levels;
pub mod viewer_menu;
pub mod viewer_view;
pub mod viewer_zoom;
pub mod watched_list;
pub mod wheel;
pub mod window;
pub mod window_layout;

#[cfg(test)]
mod tests {
    /// The state modules, with their source.
    const STATE_MODULES: [(&str, &str); 64] = [
        ("animate.rs", include_str!("animate.rs")),
        ("face_crops.rs", include_str!("face_crops.rs")),
        ("people_page.rs", include_str!("people_page.rs")),
        ("copies.rs", include_str!("copies.rs")),
        ("shortcuts.rs", include_str!("shortcuts.rs")),
        ("stats.rs", include_str!("stats.rs")),
        ("watched_list.rs", include_str!("watched_list.rs")),
        ("tag_manager.rs", include_str!("tag_manager.rs")),
        ("settings.rs", include_str!("settings.rs")),
        ("args.rs", include_str!("args.rs")),
        ("video/mod.rs", include_str!("video/mod.rs")),
        ("posters.rs", include_str!("posters.rs")),
        ("playback.rs", include_str!("playback.rs")),
        ("compare.rs", include_str!("compare.rs")),
        ("controls.rs", include_str!("controls.rs")),
        ("crop.rs", include_str!("crop.rs")),
        ("crop_tool.rs", include_str!("crop_tool.rs")),
        ("scans.rs", include_str!("scans.rs")),
        ("status.rs", include_str!("status.rs")),
        ("dialogs.rs", include_str!("dialogs.rs")),
        ("dirs.rs", include_str!("dirs.rs")),
        ("empty.rs", include_str!("empty.rs")),
        ("export_dialog.rs", include_str!("export_dialog.rs")),
        ("folder_drop.rs", include_str!("folder_drop.rs")),
        ("menu.rs", include_str!("menu.rs")),
        ("nav.rs", include_str!("nav.rs")),
        ("rename.rs", include_str!("rename.rs")),
        ("search_box.rs", include_str!("search_box.rs")),
        ("search_help.rs", include_str!("search_help.rs")),
        ("select.rs", include_str!("select.rs")),
        ("selection.rs", include_str!("selection.rs")),
        ("side_menu.rs", include_str!("side_menu.rs")),
        ("slideshow.rs", include_str!("slideshow.rs")),
        ("slideshow_order.rs", include_str!("slideshow_order.rs")),
        ("sidebar/folders.rs", include_str!("sidebar/folders.rs")),
        ("sidebar/list.rs", include_str!("sidebar/list.rs")),
        ("sidebar/rows.rs", include_str!("sidebar/rows.rs")),
        ("tag_picker.rs", include_str!("tag_picker.rs")),
        ("person_picker.rs", include_str!("person_picker.rs")),
        ("tasks.rs", include_str!("tasks.rs")),
        ("theme/tokens.rs", include_str!("theme/tokens.rs")),
        ("grid/labels.rs", include_str!("grid/labels.rs")),
        ("grid/layout.rs", include_str!("grid/layout.rs")),
        ("grid/timeline.rs", include_str!("grid/timeline.rs")),
        ("grid/motion.rs", include_str!("grid/motion.rs")),
        ("grid/glide.rs", include_str!("grid/glide.rs")),
        ("grid/scroll.rs", include_str!("grid/scroll.rs")),
        ("grid/visible.rs", include_str!("grid/visible.rs")),
        ("probe.rs", include_str!("probe.rs")),
        ("thumbs/loader.rs", include_str!("thumbs/loader.rs")),
        ("thumbs/textures.rs", include_str!("thumbs/textures.rs")),
        ("tile_menu.rs", include_str!("tile_menu.rs")),
        ("toasts.rs", include_str!("toasts.rs")),
        ("viewer.rs", include_str!("viewer.rs")),
        ("viewer_caption.rs", include_str!("viewer_caption.rs")),
        ("viewer_faces.rs", include_str!("viewer_faces.rs")),
        ("viewer_menu.rs", include_str!("viewer_menu.rs")),
        ("viewer_histogram.rs", include_str!("viewer_histogram.rs")),
        ("viewer_info.rs", include_str!("viewer_info.rs")),
        ("viewer_layout.rs", include_str!("viewer_layout.rs")),
        ("viewer_levels.rs", include_str!("viewer_levels.rs")),
        ("viewer_zoom.rs", include_str!("viewer_zoom.rs")),
        ("wheel.rs", include_str!("wheel.rs")),
        ("window_layout.rs", include_str!("window_layout.rs")),
    ];

    // A state module that reaches for egui can no longer be tested without a context, and
    // the next one copies it. Comments may name egui; code may not.
    #[test]
    fn state_modules_name_no_egui_type() {
        for (name, source) in STATE_MODULES {
            for (number, line) in source.lines().enumerate() {
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }
                assert!(
                    !code.contains("egui") && !code.contains("eframe"),
                    "{name}:{}: a state module names egui: {code}",
                    number + 1
                );
            }
        }
    }

    /// The code of every file under `dir`, comments and tests left out, as one text: a
    /// file is read to its first `#[cfg(test)]`, where this crate's tests begin.
    fn sources(dir: &std::path::Path, into: &mut String) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, into);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                for line in std::fs::read_to_string(&path).unwrap().lines() {
                    let code = line.trim_start();
                    if code.starts_with("#[cfg(test)]") {
                        break;
                    }
                    if !code.starts_with("//") {
                        into.push_str(line);
                        into.push('\n');
                    }
                }
            }
        }
    }

    /// Whether `code` names `commands::{name}` as a whole name: called, or handed on.
    fn names_command(code: &str, name: &str) -> bool {
        let path = format!("commands::{name}");
        code.match_indices(&path).any(|(at, _)| {
            (code[at + path.len()..].chars().next())
                .is_none_or(|next| !(next.is_alphanumeric() || next == '_'))
        })
    }

    // Every command the engine offers is one this window calls: the successor of the test
    // that held the Svelte UI's mock to its API. A command nobody calls is the engine's
    // dead weight, or a feature this window has not got - which the switch-over's audit
    // was about - and is deleted, or answered, rather than left.
    #[test]
    fn every_command_of_the_engine_is_called_by_the_window() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let commands = include_str!("../../photon-engine/src/commands.rs");
        let mut window = String::new();
        sources(&manifest.join("src"), &mut window);
        // By its path alone: a method of the window's own with a command's name
        // (`App::viewer_item`) is no call of the command.
        let uncalled: Vec<&str> = (commands.lines())
            .filter_map(|line| line.strip_prefix("pub fn "))
            .filter_map(|rest| rest.split(['(', '<']).next())
            .filter(|name| !names_command(&window, name))
            .collect();
        assert!(uncalled.is_empty(), "commands nothing calls: {uncalled:?}");
    }

    #[test]
    fn a_command_called_only_by_a_test_is_not_called_by_the_window() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.rs"),
            "fn draw() { commands::star(&e); }\n#[cfg(test)]\nmod tests { fn t() { commands::hide(&e); } }\n",
        )
        .unwrap();
        let mut code = String::new();
        sources(dir.path(), &mut code);
        assert!(names_command(&code, "star"));
        assert!(!names_command(&code, "hide"));
    }

    #[test]
    fn a_command_is_named_by_its_whole_path() {
        assert!(names_command("commands::adopt(&engine)", "adopt"));
        assert!(names_command("go(commands::adopt)", "adopt"));
        assert!(!names_command(
            "fn adopt(&mut self) { self.adopt(x) }",
            "adopt"
        ));
        assert!(!names_command("commands::adopted(&engine)", "adopt"));
    }
}
