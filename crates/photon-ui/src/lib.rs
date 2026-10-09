//! photon-ui: photon's interface drawn by egui on the GPU, over photon-engine.
//!
//! Two kinds of module. *State modules* are plain Rust that names no egui type and is
//! tested as such: geometry, motion, the scroll position, tasks, the thumbnail loader and
//! its bookkeeping, the tokens, the labels. *Views* draw a state module and turn input
//! into calls on it. `state_modules_name_no_egui_type` holds the line.

pub mod app;
pub mod args;
pub mod dirs;
pub mod events;
pub mod grid {
    pub mod header;
    pub mod labels;
    pub mod layout;
    pub mod motion;
    pub mod scroll;
    pub mod tile;
    pub mod view;
    pub mod visible;
}
pub mod icons;
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

#[cfg(test)]
mod tests {
    /// The state modules, with their source.
    const STATE_MODULES: [(&str, &str); 11] = [
        ("args.rs", include_str!("args.rs")),
        ("dirs.rs", include_str!("dirs.rs")),
        ("tasks.rs", include_str!("tasks.rs")),
        ("theme/tokens.rs", include_str!("theme/tokens.rs")),
        ("grid/labels.rs", include_str!("grid/labels.rs")),
        ("grid/layout.rs", include_str!("grid/layout.rs")),
        ("grid/motion.rs", include_str!("grid/motion.rs")),
        ("grid/scroll.rs", include_str!("grid/scroll.rs")),
        ("grid/visible.rs", include_str!("grid/visible.rs")),
        ("thumbs/loader.rs", include_str!("thumbs/loader.rs")),
        ("thumbs/textures.rs", include_str!("thumbs/textures.rs")),
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
}
