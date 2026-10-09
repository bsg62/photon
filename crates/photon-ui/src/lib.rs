//! photon-ui: photon's interface drawn by egui on the GPU, over photon-engine.
//!
//! Two kinds of module. *State modules* are plain Rust that names no egui type and is
//! tested as such: geometry, motion, the scroll position, tasks, the thumbnail loader and
//! its bookkeeping, the tokens, the labels. *Views* draw a state module and turn input
//! into calls on it. `state_modules_name_no_egui_type` holds the line.

pub mod args;
pub mod dirs;
pub mod tasks;
pub mod theme {
    pub mod tokens;
}
