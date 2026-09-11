//! Scans a folder into a library and generates all thumbnails.
//! Usage: cargo run --release --example index -- <library.db> <cache-dir> <photo-folder>

use photon_core::{
    grid::GridIndex,
    library::Library,
    now_ms,
    scanner::{ScanOptions, scan_watched},
    thumbs::{ThumbCache, ThumbService, default_workers},
};
use std::{path::PathBuf, sync::Arc, time::Instant};

fn main() -> photon_core::Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(db), Some(cache), Some(folder)) = (args.next(), args.next(), args.next()) else {
        eprintln!("usage: index <library.db> <cache-dir> <photo-folder>");
        std::process::exit(2);
    };

    let lib = Arc::new(Library::open(&PathBuf::from(db))?);
    let watched = lib.add_watched_folder(&PathBuf::from(folder), &[PathBuf::from(&cache)])?;

    let started = Instant::now();
    let options = ScanOptions {
        excluded: vec![PathBuf::from(&cache)],
        ..Default::default()
    };
    let report = scan_watched(&lib, &watched, now_ms(), &options, &mut |p| {
        eprint!("\rscanned {} files", p.files_seen)
    })?;
    eprintln!("\n{report:?} in {:?}", started.elapsed());

    let started = Instant::now();
    let grid = GridIndex::build(lib.grid_entries()?);
    eprintln!(
        "grid: {} items in {} sections, built in {:?}",
        grid.len(),
        grid.sections().len(),
        started.elapsed()
    );

    let service = ThumbService::start(
        lib.clone(),
        Arc::new(ThumbCache::new(cache.clone())),
        default_workers(),
    );
    let started = Instant::now();
    let queued = service.enqueue_pending()?;
    service.wait_idle();
    eprintln!("thumbnails for {queued} items in {:?}", started.elapsed());
    Ok(())
}
