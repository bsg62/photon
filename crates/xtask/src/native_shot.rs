//! `cargo run -p xtask -- native-shot`: the native grid as two PNGs, light and dark, in
//! `target/screenshots/`.
//!
//! The rendering is a test of photon-ui (`tests/screenshots.rs`), ignored by default and
//! run from here, so that xtask does not itself depend on a GPU stack. Like `screenshots`
//! it is not run in CI and replaces no item of the smoke checklist.

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
            "screenshots",
            "--",
            "--ignored",
            "--nocapture",
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
