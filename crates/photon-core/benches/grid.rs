use criterion::{Criterion, criterion_group, criterion_main};
use photon_core::{
    grid::{GridIndex, GridView, Layout},
    library::{HashCandidate, Library, NewItem},
    media::MediaKind,
};
use std::{hint::black_box, path::Path};

/// A file size between 1 and 8 MB that changes from photo to photo with no relation to
/// insertion order, as a real library's do. With one size for every photo, size order is
/// table order, and a query that walks `items_size` runs here as fast as a scan while taking
/// seven to twelve times as long once sizes vary - which is how five of them went unnoticed
/// (see photon-core's `library/mod.rs`).
fn file_size(n: usize) -> i64 {
    // Fibonacci hashing: the high bits of n * 2^64/phi spread consecutive n across the range.
    let spread = (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32;
    1_000_000 + (spread % 7_000_000) as i64
}

/// 1,000 folders × 100 photos = 100k items, the spec's target library size.
fn synthetic_library(dir: &Path, folders: usize, per_folder: usize) -> Library {
    let lib = Library::open(&dir.join("bench.db")).unwrap();
    let root = dir.join("photos");
    std::fs::create_dir_all(&root).unwrap();
    let watched = lib.add_watched_folder(&root, &[]).unwrap();
    let root_id = lib
        .upsert_folder(watched.id, None, root.to_str().unwrap(), 1)
        .unwrap();
    let mut items = Vec::with_capacity(folders * per_folder);
    for f in 0..folders {
        let folder_path = root.join(format!("folder-{f:04}"));
        let folder_str = folder_path.to_str().unwrap().to_string();
        let folder_id = lib
            .upsert_folder(watched.id, Some(root_id), &folder_str, 1)
            .unwrap();
        for i in 0..per_folder {
            let name = format!("IMG_{i:05}.jpg");
            items.push(NewItem {
                folder_id,
                path: folder_path.join(&name).to_str().unwrap().to_string(),
                file_name: name,
                kind: MediaKind::Image,
                size: file_size(f * per_folder + i),
                mtime_ms: 1_700_000_000_000 + i as i64,
                width: 4000,
                height: 3000,
                orientation: 1,
                taken_at: 1_700_000_000 + (f * per_folder + i) as i64,
                rating: None,
                camera: photon_core::metadata::CameraMeta::default(),
                tags: Vec::new(),
                caption: None,
                duration_ms: None,
            });
        }
    }
    lib.insert_items(&items).unwrap();
    lib
}

/// A library shaped for search: 1,000 folders × 100 photos whose names, cameras, keywords,
/// captions and folder aliases vary, so a search walks every path `search_entries` has - the
/// camera haystacks, the keywords read through `EFFECTIVE_TAGS` (with a rename rule in
/// force), the caption, the alias, non-ASCII folder names - rather than 100k rows of
/// `IMG_n.jpg` with nothing else to read.
fn search_library(dir: &Path) -> Library {
    const PLACES: &[&str] = &[
        "Italy",
        "München",
        "Lake Garda",
        "Beach",
        "Paris",
        "Zürich",
        "Snow",
        "Garden",
        "Kraków",
        "Wedding",
    ];
    const CAMERAS: &[(&str, &str)] = &[
        ("Canon", "Canon EOS 5D Mark IV"),
        ("NIKON CORPORATION", "NIKON D750"),
        ("FUJIFILM", "X100V"),
        ("SONY", "ILCE-7M3"),
        ("Apple", "iPhone 13 Pro"),
    ];
    const LENSES: &[&str] = &["EF24-105mm f/4L IS USM", "50mm f/1.8", "XF23mmF2 R WR"];
    const TAGS: &[&str] = &["family", "holiday", "sunset", "Straße", "portrait", "dog"];
    let lib = Library::open(&dir.join("search.db")).unwrap();
    let root = dir.join("photos");
    std::fs::create_dir_all(&root).unwrap();
    let watched = lib.add_watched_folder(&root, &[]).unwrap();
    let root_id = lib
        .upsert_folder(watched.id, None, root.to_str().unwrap(), 1)
        .unwrap();
    let mut items = Vec::with_capacity(100_000);
    for f in 0..1_000 {
        let place = PLACES[f % PLACES.len()];
        let folder_path = root.join(format!("{} {place} {f:04}", 2000 + f % 25));
        let folder_id = lib
            .upsert_folder(watched.id, Some(root_id), folder_path.to_str().unwrap(), 1)
            .unwrap();
        if f % 10 == 0 {
            lib.set_folder_alias(folder_id, Some(&format!("Trip to {place} #{f}")))
                .unwrap();
        }
        for i in 0..100 {
            let n = f * 100 + i;
            let name = match n % 4 {
                0 => format!("IMG_{n:05}.JPG"),
                1 => format!("DSC{n:05}.jpg"),
                2 => format!("{place} {i}.jpg"),
                _ => format!("P{n:07}.jpeg"),
            };
            let camera = if n % 5 < 3 {
                let (make, model) = CAMERAS[n % CAMERAS.len()];
                photon_core::metadata::CameraMeta {
                    make: Some(make.to_string()),
                    model: Some(model.to_string()),
                    lens: (n % 5 < 2).then(|| LENSES[n % LENSES.len()].to_string()),
                    focal_mm: Some(18.0 + (n % 90) as f64),
                    aperture: Some([1.8, 2.8, 4.0, 5.6][n % 4]),
                    exposure_s: Some(1.0 / 250.0),
                    iso: Some([100, 200, 400, 1600][n % 4]),
                }
            } else {
                photon_core::metadata::CameraMeta::default()
            };
            let tags = if n % 10 < 3 {
                (0..1 + n % 3)
                    .map(|k| TAGS[(n + k) % TAGS.len()].to_string())
                    .collect()
            } else {
                Vec::new()
            };
            items.push(NewItem {
                folder_id,
                path: folder_path.join(&name).to_str().unwrap().to_string(),
                file_name: name,
                kind: if n % 50 == 0 {
                    MediaKind::Video
                } else {
                    MediaKind::Image
                },
                size: file_size(n),
                mtime_ms: 1_700_000_000_000 + i as i64,
                width: 4000,
                height: 3000,
                orientation: 1,
                taken_at: 946_684_800 + (n as i64) * 7_919,
                rating: None,
                camera,
                tags,
                caption: (n % 20 == 0).then(|| format!("A day at the {place}\nwith friends")),
                duration_ms: None,
            });
        }
    }
    lib.insert_items(&items).unwrap();
    lib.rename_tag("dog", "Dogs").unwrap();
    lib
}

fn bench_grid(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let lib = synthetic_library(dir.path(), 1_000, 100);

    // Spec budget: warm startup to first grid data < 1s.
    c.bench_function("startup_grid_100k", |b| {
        b.iter(|| {
            black_box(GridIndex::build(
                lib.grid_entries().unwrap(),
                Layout::Folders,
            ))
        })
    });

    // Spec budget: grid_rows page query < 50ms.
    let index = GridIndex::build(lib.grid_entries().unwrap(), Layout::Folders);
    c.bench_function("grid_rows_page", |b| {
        b.iter(|| black_box(index.rows(black_box(50_000), 200).len()))
    });

    // The same library with one photo in ten a byte-identical pair, so the grid query's
    // `has_copies` column has a real set to build and probe; the library above has no
    // hashes at all and would measure it empty. Same budget as startup.
    let dup_dir = tempfile::tempdir().unwrap();
    let dup_lib = synthetic_library(dup_dir.path(), 1_000, 100);
    let rows = dup_lib.grid_entries().unwrap();
    for (n, entry) in rows.iter().enumerate().filter(|(n, _)| n % 10 < 1) {
        let item = dup_lib.item(entry.id).unwrap().unwrap();
        let candidate = HashCandidate {
            id: item.id,
            path: item.path,
            size: item.size,
            mtime_ms: item.mtime_ms,
        };
        // Pairs: rows 0 and 10 share a hash, 20 and 30, and so on. Real copies would share
        // a size as well; these need not, because `has_copies` reads only the hash. Checked,
        // because the store is guarded by each row's own size and mtime, and a refusal here
        // would leave the set empty and the benchmark measuring the library above again.
        let stored = dup_lib
            .set_content_hash(&candidate, &((n / 20) as u128).to_le_bytes())
            .unwrap();
        assert!(stored, "row {n} refused its hash");
    }
    c.bench_function("startup_grid_100k_with_duplicates", |b| {
        b.iter(|| {
            black_box(GridIndex::build(
                dup_lib.grid_entries().unwrap(),
                Layout::Folders,
            ))
        })
    });
}

/// One debounced keystroke's search over 100k photos. `beach` is a word most rows lack, so
/// nearly every row is read, folded and rejected - the common case while typing; the second
/// query adds a prefixed term and a non-ASCII word.
fn bench_search(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let lib = search_library(dir.path());
    // Checked, so a broken library cannot turn this into a benchmark of an empty result.
    assert!(
        !lib.entries_for(GridView::Search, "beach")
            .unwrap()
            .is_empty()
    );
    c.bench_function("search_100k", |b| {
        b.iter(|| {
            black_box(
                lib.entries_for(GridView::Search, black_box("beach"))
                    .unwrap(),
            )
        })
    });
    c.bench_function("search_100k_camera_and_word", |b| {
        b.iter(|| {
            black_box(
                lib.entries_for(GridView::Search, black_box("camera:canon münchen"))
                    .unwrap(),
            )
        })
    });
}

criterion_group!(benches, bench_grid, bench_search);
criterion_main!(benches);
