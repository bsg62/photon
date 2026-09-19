//! Repository chores that need to run identically in CI and on a developer machine.
//!
//! Usage:
//!   cargo run -p xtask -- versions [--tag v0.1.0]
//!   cargo run -p xtask -- metadata
//!   cargo run -p xtask -- screenshots [--out <dir>] [--only <shot>] [--no-build]

mod checks;
mod screenshots;

use checks::{
    Versions, check_metadata, check_versions, parse_cargo_version, parse_pkg_version,
    parse_tauri_version,
};
use std::{path::Path, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str);
    let tag = tag_arg(&args);
    match command {
        Some("versions") => run_versions(tag.as_deref()),
        Some("metadata") => run_metadata(),
        Some("screenshots") => screenshots::run(&repo_root(), &args),
        other => {
            eprintln!(
                "unknown command {other:?}; expected `versions`, `metadata` or `screenshots`"
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
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("xtask lives at <root>/crates/xtask")
        .to_path_buf();

    let read = |rel: &str| -> Result<String, String> {
        std::fs::read_to_string(root.join(rel)).map_err(|e| format!("cannot read {rel}: {e}"))
    };

    let parsed = (|| -> Result<Versions, String> {
        Ok(Versions {
            tauri: parse_tauri_version(&read("crates/photon-app/tauri.conf.json")?)?,
            cargo: parse_cargo_version(&read("Cargo.toml")?)?,
            ui: parse_pkg_version(&read("ui/package.json")?)?,
        })
    })();

    let versions = match parsed {
        Ok(v) => v,
        Err(err) => {
            eprintln!("version check could not run: {err}");
            return ExitCode::FAILURE;
        }
    };

    match check_versions(&versions, tag) {
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
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("xtask lives at <root>/crates/xtask")
        .to_path_buf();

    let cargo_toml = match std::fs::read_to_string(root.join("Cargo.toml")) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read Cargo.toml: {e}");
            return ExitCode::FAILURE;
        }
    };
    let conf_path = root
        .join("crates")
        .join("photon-app")
        .join("tauri.conf.json");
    let tauri_conf = match std::fs::read_to_string(&conf_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read tauri.conf.json: {e}");
            return ExitCode::FAILURE;
        }
    };

    match check_metadata(&cargo_toml, &tauri_conf, root.join("LICENSE").is_file()) {
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
