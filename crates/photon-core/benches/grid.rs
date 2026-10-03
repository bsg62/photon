use criterion::{Criterion, criterion_group, criterion_main};
use photon_core::{
    grid::{GridIndex, GridView, Layout},
    keywords::keywords_in,
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
                    gps: None,
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

    // The Starred view with one photo in thirty-three starred, about a real library's share.
    // Served by `items_starred`, it reads the starred rows alone; through `items_folder`, as
    // it was before schema 22, it read the whole library twice to return 3% of it.
    let stars: Vec<(i64, u8)> = lib
        .grid_entries()
        .unwrap()
        .iter()
        .step_by(33)
        .map(|entry| (entry.id, 1))
        .collect();
    lib.set_ratings(&stars).unwrap();
    c.bench_function("startup_grid_100k_starred", |b| {
        b.iter(|| {
            black_box(GridIndex::build(
                lib.entries_for(GridView::Starred, "").unwrap(),
                Layout::Folders,
            ))
        })
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

/// A library shaped for the sidebar's tag counts: 1,000 folders × 100 photos, every photo
/// carrying three keywords out of 300, with each thing `EFFECTIVE_TAGS` and the counts treat
/// specially present - hidden and missing photos, tags added and removed on single photos,
/// and a rename, a merge and a hidden tag among the rules. The search library tags a third
/// of its photos from six names, which is not the case the counts are slow in.
fn tag_library(dir: &Path) -> Library {
    let lib = Library::open(&dir.join("tags.db")).unwrap();
    let root = dir.join("photos");
    std::fs::create_dir_all(&root).unwrap();
    let watched = lib.add_watched_folder(&root, &[]).unwrap();
    let root_id = lib
        .upsert_folder(watched.id, None, root.to_str().unwrap(), 1)
        .unwrap();
    let keyword = |n: usize, k: usize| format!("tag{:03}", (n * 7 + k * 101) % 300);
    let mut items = Vec::with_capacity(100_000);
    for f in 0..1_000 {
        let folder_path = root.join(format!("folder-{f:04}"));
        let folder_id = lib
            .upsert_folder(watched.id, Some(root_id), folder_path.to_str().unwrap(), 1)
            .unwrap();
        for i in 0..100 {
            let n = f * 100 + i;
            let name = format!("IMG_{n:06}.jpg");
            items.push(NewItem {
                folder_id,
                path: folder_path.join(&name).to_str().unwrap().to_string(),
                file_name: name,
                kind: MediaKind::Image,
                size: file_size(n),
                mtime_ms: 1_700_000_000_000 + n as i64,
                width: 4000,
                height: 3000,
                orientation: 1,
                taken_at: 1_000_000_000 + n as i64 * 600,
                rating: None,
                camera: photon_core::metadata::CameraMeta::default(),
                tags: (0..3).map(|k| keyword(n, k)).collect(),
                caption: None,
                duration_ms: None,
            });
        }
    }
    let ids = lib.insert_items(&items).unwrap();
    let every = |step: usize, from: usize| -> Vec<i64> {
        ids.iter().copied().skip(from).step_by(step).collect()
    };
    lib.set_hidden(&every(50, 3), true).unwrap();
    lib.mark_missing(&every(100, 7), 1).unwrap();
    lib.add_items_tag(&every(100, 11), "added by hand").unwrap();
    lib.add_items_tag(&every(200, 13), "tag010").unwrap();
    for (n, id) in ids.iter().enumerate().skip(17).step_by(200) {
        lib.remove_item_tag(*id, &keyword(n, 1)).unwrap();
    }
    lib.rename_tag("tag001", "renamed").unwrap();
    lib.rename_tag("tag002", "tag003").unwrap(); // a merge: some photos carry both
    lib.hide_tag("tag004").unwrap();
    lib
}

fn bench_tags(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let lib = tag_library(dir.path());
    // Checked, so the library is the one described: the merge is in force and the hidden
    // photos make some tag's shown count differ from its total.
    let tags = lib.tags_with_counts().unwrap();
    assert!(tags.iter().any(|t| t.tag == "renamed"));
    assert!(!tags.iter().any(|t| t.tag == "tag002"));
    assert!(tags.iter().any(|t| t.count < t.total));
    c.bench_function("tag_counts_100k", |b| {
        b.iter(|| black_box(lib.tags_with_counts().unwrap()))
    });
}

/// 150,000 detected faces with vectors on the 100k-photo library, in 30,000 groups shaped
/// like a first recognition: 20 named people of 1,500 faces (1,200 confirmed, 300
/// suggestions), 30 unnamed groups of 500, 4,950 of 13, 15,000 pairs (100 of them ignored)
/// and 10,000 single faces, plus 650 faces ignored one by one. A group's faces are spread
/// over the library, as one person's are, and 1% of the photos are hidden.
///
/// Written straight into `library.db` in one transaction, as the grouping step would leave
/// it: running the detector, the embedder and the grouping over 100k photos to get here would
/// take hours, and none of it is what is measured. No Picasa faces, so no group has an offer
/// to work out; that cost falls on the 200 listed groups' photos alone.
fn people_library(dir: &Path) -> Library {
    let lib = synthetic_library(dir, 1_000, 100);
    let ids: Vec<i64> = {
        let conn = rusqlite::Connection::open(dir.join("bench.db")).unwrap();
        let mut stmt = conn.prepare("SELECT id FROM items ORDER BY id").unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    lib.set_face_detection(true).unwrap();
    let mut conn = rusqlite::Connection::open(dir.join("bench.db")).unwrap();
    let tx = conn.transaction().unwrap();
    {
        let mut person = tx
            .prepare("INSERT INTO people (name, ignored) VALUES (?1, ?2)")
            .unwrap();
        let mut face = tx
            .prepare(
                "INSERT INTO detected_faces (item_id, left, top, right, bottom, landmarks,
                     score, embedding, embedding_version, person_id, confirmed, ignored)
                 VALUES (?1, 0.4, 0.4, 0.5, 0.5, ?2, 0.9, ?3, 1, ?4, ?5, ?6)",
            )
            .unwrap();
        let landmarks = vec![0u8; 40];
        let embedding = vec![0u8; 512];
        let mut n = 0usize;
        // Face n is on photo n * 7919 mod the library: a group's faces are spread out.
        let mut add = |group: Option<i64>, confirmed: bool, ignored: bool| {
            let item = ids[(n * 7_919) % ids.len()];
            n += 1;
            face.execute(rusqlite::params![
                item, landmarks, embedding, group, confirmed, ignored
            ])
            .unwrap();
        };
        let mut new_group = |name: Option<String>, ignored: bool| {
            person.execute(rusqlite::params![name, ignored]).unwrap();
            tx.last_insert_rowid()
        };
        let mut shape: Vec<(Option<String>, bool, usize)> = Vec::new();
        shape.extend((0..20).map(|i| (Some(format!("Person {i:02}")), false, 1_500)));
        shape.extend((0..30).map(|_| (None, false, 500)));
        shape.extend((0..4_950).map(|_| (None, false, 13)));
        shape.extend((0..15_000).map(|i| (None, i < 100, 2)));
        shape.extend((0..10_000).map(|_| (None, false, 1)));
        for (name, ignored, size) in shape {
            let named = name.is_some();
            let group = new_group(name, ignored);
            for k in 0..size {
                add(Some(group), named && k < 1_200, false);
            }
        }
        for _ in 0..650 {
            add(None, false, true);
        }
        assert_eq!(n, 150_000);
    }
    tx.commit().unwrap();
    lib.set_hidden(&ids.iter().copied().step_by(100).collect::<Vec<_>>(), true)
        .unwrap();
    lib
}

fn bench_people(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let lib = people_library(dir.path());
    // Checked, so the library is the one described: the Unnamed section is past its cap, and
    // every section has something in it.
    let page = lib.people_page(12).unwrap();
    assert_eq!(page.unnamed.len(), 200);
    assert!(page.unnamed_count > 15_000, "{}", page.unnamed_count);
    assert_eq!(lib.people_to_name().unwrap(), page.unnamed_count);
    assert_eq!(page.people.len(), 20);
    assert!(page.single_count > 9_000 && !page.ignored_groups.is_empty());
    let mut group = c.benchmark_group("people_150k_faces");
    // Each read is tens of milliseconds or more: the default hundred samples would make
    // this the longest bench here for no more certainty than the gap between the two needs.
    group.sample_size(10);
    group.bench_function("people_page", |b| {
        b.iter(|| black_box(lib.people_page(12).unwrap()))
    });
    group.bench_function("people_to_name", |b| {
        b.iter(|| black_box(lib.people_to_name().unwrap()))
    });
    group.finish();
}

/// `len` bytes that look like compressed image data: no structure, and invalid UTF-8
/// throughout, which is what surrounds an XMP packet in a real file.
fn noise(len: usize, seed: &mut u32) -> Vec<u8> {
    (0..len)
        .map(|_| {
            *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (*seed >> 24) as u8
        })
        .collect()
}

/// The keyword read the scanner makes for every new or changed photo, over a full
/// `xmp::MAX_PREFIX`: once with no packet (most camera originals), and once with a packet
/// near the start followed by image data (phones and editors). With no packet the whole
/// prefix is searched for the start marker and nothing is found, which is where the search
/// itself is the cost.
fn bench_keywords(c: &mut Criterion) {
    let mut seed = 7;
    let without = noise(photon_core::xmp::MAX_PREFIX, &mut seed);
    let mut with = noise(2_000, &mut seed);
    with.extend_from_slice(
        br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF><rdf:Description><dc:subject><rdf:Bag><rdf:li>beach</rdf:li><rdf:li>summer</rdf:li></rdf:Bag></dc:subject></rdf:Description></rdf:RDF></x:xmpmeta>"#,
    );
    with.extend(noise(photon_core::xmp::MAX_PREFIX - with.len(), &mut seed));
    // The same honesty check as the search benches: each input must be the case it is
    // named for.
    assert_eq!(keywords_in(&with), ["beach", "summer"]);
    assert!(keywords_in(&without).is_empty());

    c.bench_function("keywords_256k_without_xmp", |b| {
        b.iter(|| black_box(keywords_in(black_box(&without))))
    });
    c.bench_function("keywords_256k_with_xmp", |b| {
        b.iter(|| black_box(keywords_in(black_box(&with))))
    });
}

criterion_group!(
    benches,
    bench_grid,
    bench_search,
    bench_tags,
    bench_people,
    bench_keywords
);
criterion_main!(benches);
