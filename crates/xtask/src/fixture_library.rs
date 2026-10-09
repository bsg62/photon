//! `cargo run -p xtask -- fixture-library [--photos N] [--out DIR]`: the library the gate
//! measures both applications over, by itself.
//!
//! The builder is photon-ui's (`fixture.rs`, run as its `fixture` example), so that xtask
//! does not itself depend on photon-core. Release, because 300,000 rows in a debug build
//! is a wait.

use std::{
    path::Path,
    process::{Command, ExitCode},
};

pub fn run(root: &Path, args: &[String]) -> ExitCode {
    let mut passed: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    if !passed.contains(&"--out") {
        passed.extend(["--out", "target/gate-fixture"]);
    }
    let status = Command::new(env!("CARGO"))
        .current_dir(root)
        .args([
            "run",
            "--release",
            "-p",
            "photon-ui",
            "--example",
            "fixture",
            "--",
        ])
        .args(passed)
        .status();
    match status {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("cannot run cargo: {err}");
            ExitCode::FAILURE
        }
    }
}
