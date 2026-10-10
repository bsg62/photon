//! photon-ui: photon's interface drawn by egui on the GPU, over photon-engine.
//!
//! Two kinds of module. *State modules* are plain Rust that names no egui type and is
//! tested as such: geometry, motion, the scroll position, tasks, the thumbnail loader and
//! its bookkeeping, the tokens, the labels. *Views* draw a state module and turn input
//! into calls on it. `state_modules_name_no_egui_type` holds the line.

pub mod app;
pub mod args;
pub mod controls;
pub mod controls_bar;
pub mod dirs;
pub mod empty;
pub mod events;
pub mod fixture;
pub mod grid {
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
pub mod nav;
pub mod probe;
pub mod scans;
pub mod search_bar;
pub mod search_box;
pub mod search_help;
pub mod select;
pub mod select_view;
pub mod shell;
pub mod status;
pub mod sidebar {
    pub mod folders;
    pub mod list;
    pub mod rows;
    pub mod view;
}
pub mod tasks;
pub mod text;
pub mod theme {
    pub mod apply;
    pub mod fonts;
    pub mod tokens;
}
pub mod thumbs {
    pub mod loader;
    pub mod shown;
    pub mod source;
    pub mod textures;
}
pub mod toasts;
pub mod window_layout;

#[cfg(test)]
mod tests {
    /// The state modules, with their source.
    const STATE_MODULES: [(&str, &str); 26] = [
        ("args.rs", include_str!("args.rs")),
        ("controls.rs", include_str!("controls.rs")),
        ("scans.rs", include_str!("scans.rs")),
        ("status.rs", include_str!("status.rs")),
        ("dirs.rs", include_str!("dirs.rs")),
        ("empty.rs", include_str!("empty.rs")),
        ("nav.rs", include_str!("nav.rs")),
        ("search_box.rs", include_str!("search_box.rs")),
        ("search_help.rs", include_str!("search_help.rs")),
        ("select.rs", include_str!("select.rs")),
        ("sidebar/folders.rs", include_str!("sidebar/folders.rs")),
        ("sidebar/list.rs", include_str!("sidebar/list.rs")),
        ("sidebar/rows.rs", include_str!("sidebar/rows.rs")),
        ("tasks.rs", include_str!("tasks.rs")),
        ("theme/tokens.rs", include_str!("theme/tokens.rs")),
        ("grid/labels.rs", include_str!("grid/labels.rs")),
        ("grid/layout.rs", include_str!("grid/layout.rs")),
        ("grid/timeline.rs", include_str!("grid/timeline.rs")),
        ("grid/motion.rs", include_str!("grid/motion.rs")),
        ("grid/scroll.rs", include_str!("grid/scroll.rs")),
        ("grid/visible.rs", include_str!("grid/visible.rs")),
        ("probe.rs", include_str!("probe.rs")),
        ("thumbs/loader.rs", include_str!("thumbs/loader.rs")),
        ("thumbs/textures.rs", include_str!("thumbs/textures.rs")),
        ("toasts.rs", include_str!("toasts.rs")),
        ("window_layout.rs", include_str!("window_layout.rs")),
    ];

    /// The version of the `windows` crate that `package` is locked to, or `None` when it
    /// does not depend on it.
    fn locked_windows_crate(lock: &str, package: &str) -> Option<String> {
        let entry = lock.split("[[package]]").find(|entry| {
            // By line, so a checkout with CRLF endings reads the same.
            let name = format!("name = \"{package}\"");
            entry.lines().any(|line| line.trim() == name)
        })?;
        let dependency = entry
            .lines()
            .map(str::trim)
            .find_map(|line| line.strip_prefix("\"windows "))?;
        Some(dependency.trim_end_matches(['"', ',']).to_owned())
    }

    // gpu-allocator takes any `windows` crate from 0.53 to 0.62, and wgpu-hal exactly 0.62.
    // Tauri, in the same lockfile until the switch-over, brings 0.61 - and cargo, adding
    // photon-ui to a lock that already held 0.61, gave gpu-allocator that one. Its Direct3D
    // types are then not wgpu-hal's, and wgpu-hal does not compile: on Windows only, four
    // minutes into CI, as ten errors about `ID3D12Heap` that name neither crate's version.
    // A `cargo update` can do it again; this says so on every platform, by name. The cure
    // is to change gpu-allocator's `"windows 0.61.x"` line in Cargo.lock to wgpu-hal's.
    #[test]
    fn the_gpu_crates_are_locked_to_one_windows_crate() {
        let lock = include_str!("../../../Cargo.lock");
        let wgpu = locked_windows_crate(lock, "wgpu-hal");
        assert!(
            wgpu.is_some(),
            "wgpu-hal no longer depends on the windows crate"
        );
        assert_eq!(locked_windows_crate(lock, "gpu-allocator"), wgpu);
    }

    // A Windows checkout has CRLF line endings, and `include_str!` hands them over as they
    // are: the first version of this reader matched a name with its `\n` and found nothing
    // there, on the one platform the test is about.
    #[test]
    fn the_lockfile_is_read_with_either_line_ending() {
        let lock = include_str!("../../../Cargo.lock").replace("\r\n", "\n");
        let unix = locked_windows_crate(&lock, "wgpu-hal");
        assert!(unix.is_some());
        let windows = lock.replace('\n', "\r\n");
        assert_eq!(locked_windows_crate(&windows, "wgpu-hal"), unix);
    }

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
}
