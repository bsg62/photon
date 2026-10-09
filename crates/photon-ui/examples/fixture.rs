//! Builds the library the gate measures both applications over (`photon_ui::fixture`).
//!
//!   cargo run --release -p photon-ui --example fixture -- --photos 300000 --out target/gate-fixture
//!
//! The thumbnails are made from the CC0 photos the screenshots use, unless `--sources`
//! names another directory of JPEGs.

use photon_ui::fixture;
use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let mut photos = 300_000usize;
    let mut out = None;
    let mut sources = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../xtask/screenshots/photos");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args.next();
        match (arg.as_str(), value) {
            ("--photos", Some(value)) => match value.parse() {
                Ok(number) => photos = number,
                Err(_) => return usage(&format!("--photos {value} is not a number")),
            },
            ("--out", Some(value)) => out = Some(PathBuf::from(value)),
            ("--sources", Some(value)) => sources = PathBuf::from(value),
            (other, _) => return usage(&format!("unknown or incomplete argument {other}")),
        }
    }
    let Some(out) = out else {
        return usage("--out is needed");
    };
    let mut pictures: Vec<PathBuf> = match std::fs::read_dir(&sources) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "jpg"))
            .collect(),
        Err(err) => return usage(&format!("{}: {err}", sources.display())),
    };
    pictures.sort();
    match fixture::build(&out, photos, &pictures) {
        Ok(fixture) => {
            println!("{} photos in {}", fixture.photos, out.display());
            println!("  --data-dir {}", fixture.data_dir.display());
            println!("  --cache-dir {}", fixture.cache_dir.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("could not build the fixture: {err}");
            ExitCode::FAILURE
        }
    }
}

fn usage(problem: &str) -> ExitCode {
    eprintln!("{problem}\n\nusage: fixture --out DIR [--photos N] [--sources DIR]");
    ExitCode::from(2)
}
