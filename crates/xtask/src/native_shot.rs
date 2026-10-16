//! `cargo run -p xtask -- native-shot`: the native window as PNGs, in
//! `target/screenshots/native-*.png`.
//!
//! The rendering is a test of photon-ui (`tests/screenshots.rs`), ignored by default and
//! run from here, so that xtask does not itself depend on a GPU stack. Through `cargo test`,
//! which runs them in one process, and not through nextest, which runs a process for each:
//! the pictures make their GPU devices one at a time by a lock of the process's own
//! (`DEVICES`), and two dozen made at once crashed the Vulkan loader. It is not run in CI
//! and replaces no item of the smoke checklist.

use std::{
    path::Path,
    process::{Command, ExitCode},
};

pub fn run(root: &Path) -> ExitCode {
    let status = Command::new(env!("CARGO"))
        .current_dir(root)
        .args([
            "test",
            "-p",
            "photon-ui",
            "--test",
            "all",
            "--",
            "--ignored",
            "--nocapture",
            "screenshots::",
        ])
        .status();
    match status {
        Ok(status) if status.success() => {
            println!("{}", root.join("target").join("screenshots").display());
            ExitCode::SUCCESS
        }
        Ok(_) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("cannot run cargo: {err}");
            ExitCode::FAILURE
        }
    }
}
