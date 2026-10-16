//! Repository chores that need to run identically in CI and on a developer machine.
//!
//! Usage:
//!   cargo run -p xtask -- versions [--tag v0.1.0]
//!   cargo run -p xtask -- metadata
//!   cargo run -p xtask -- native-shot
//!   cargo run -p xtask -- fixture-library [--photos <n>] [--out <dir>]
//!   cargo run -p xtask -- package (deb | appimage | dmg --arch <a> | msi) --binary <path> --out <dir>

mod checks;
mod fixture_library;
mod native_shot;
mod package;

use checks::{check_metadata, check_versions, parse_cargo_version};
use std::{path::Path, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str);
    let tag = tag_arg(&args);
    match command {
        Some("versions") => run_versions(tag.as_deref()),
        Some("metadata") => run_metadata(),
        Some("native-shot") => native_shot::run(&repo_root()),
        Some("fixture-library") => fixture_library::run(&repo_root(), &args),
        Some("package") => package::run(&repo_root(), &args),
        other => {
            eprintln!(
                "unknown command {other:?}; expected `versions`, `metadata`, `native-shot`, `fixture-library` or `package`"
            );
            ExitCode::FAILURE
        }
    }
}

fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("xtask lives at <root>/crates/xtask")
        .to_path_buf()
}

/// Reads `--tag <value>`. An empty value is treated as absent, because a
/// `workflow_dispatch` run passes an empty string rather than omitting the flag.
fn tag_arg(args: &[String]) -> Option<String> {
    let i = args.iter().position(|a| a == "--tag")?;
    let value = args.get(i + 1)?;
    if value.is_empty() {
        None
    } else {
        Some(value.clone())
    }
}

fn run_versions(tag: Option<&str>) -> ExitCode {
    let version = std::fs::read_to_string(repo_root().join("Cargo.toml"))
        .map_err(|e| format!("cannot read Cargo.toml: {e}"))
        .and_then(|text| parse_cargo_version(&text));
    let version = match version {
        Ok(version) => version,
        Err(err) => {
            eprintln!("version check could not run: {err}");
            return ExitCode::FAILURE;
        }
    };
    match check_versions(&version, tag) {
        Ok(version) => {
            println!("version check passed: {version}");
            ExitCode::SUCCESS
        }
        Err(problems) => {
            eprintln!("version check failed:");
            for p in &problems {
                eprintln!("  - {p}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run_metadata() -> ExitCode {
    let root = repo_root();
    let cargo_toml = match std::fs::read_to_string(root.join("Cargo.toml")) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read Cargo.toml: {e}");
            return ExitCode::FAILURE;
        }
    };
    match check_metadata(&cargo_toml, root.join("LICENSE").is_file()) {
        Ok(()) => {
            println!("metadata check passed");
            ExitCode::SUCCESS
        }
        Err(problems) => {
            eprintln!("metadata check failed:");
            for p in &problems {
                eprintln!("  - {p}");
            }
            ExitCode::FAILURE
        }
    }
}
