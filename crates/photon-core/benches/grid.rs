use criterion::{Criterion, criterion_group, criterion_main};
use photon_core::{
    grid::GridIndex,
    library::{Library, NewItem},
    media::MediaKind,
};
use std::{hint::black_box, path::Path};

/// 1,000 folders × 100 photos = 100k items, the spec's target library size.
fn synthetic_library(dir: &Path, folders: usize, per_folder: usize) -> Library {
    let lib = Library::open(&dir.join("bench.db")).unwrap();
    let root = dir.join("photos");
    let watched = lib.add_watched_folder(&root).unwrap();
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
                size: 4_000_000,
                mtime_ms: 1_700_000_000_000 + i as i64,
                width: 4000,
                height: 3000,
                orientation: 1,
                taken_at: 1_700_000_000 + (f * per_folder + i) as i64,
            });
        }
    }
    lib.insert_items(&items).unwrap();
    lib
}

fn bench_grid(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let lib = synthetic_library(dir.path(), 1_000, 100);

    // Spec budget: warm startup to first grid data < 1s.
    c.bench_function("startup_grid_100k", |b| {
        b.iter(|| black_box(GridIndex::build(lib.grid_entries().unwrap())))
    });

    // Spec budget: grid_rows page query < 50ms.
    let index = GridIndex::build(lib.grid_entries().unwrap());
    c.bench_function("grid_rows_page", |b| {
        b.iter(|| black_box(index.rows(black_box(50_000), 200).len()))
    });
}

criterion_group!(benches, bench_grid);
criterion_main!(benches);
