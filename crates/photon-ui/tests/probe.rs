//! The gate's fixture: a library built to be measured.

use photon_core::{library::Library, media::ThumbState};
use photon_ui::fixture;
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

/// Three small JPEGs to make thumbnails from.
fn sources(dir: &Path) -> Vec<PathBuf> {
    [(64, 48), (48, 64), (96, 64)]
        .iter()
        .enumerate()
        .map(|(n, &(width, height))| {
            let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                width,
                height,
                image::Rgb([40 * n as u8, 120, 200]),
            ));
            let mut bytes = Vec::new();
            image
                .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
                .unwrap();
            let path = dir.join(format!("source-{n}.jpg"));
            std::fs::write(&path, bytes).unwrap();
            path
        })
        .collect()
}

#[test]
fn a_fixture_is_a_library_with_every_thumbnail_cached_and_no_folder_to_scan() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = fixture::build(&dir.path().join("fixture"), 700, &sources(dir.path())).unwrap();
    let dirs = fixture.dirs();

    let lib = Library::open(&dirs.db_path).unwrap();
    let entries = lib.grid_entries().unwrap();
    assert_eq!(entries.len(), 700);
    // Three folders: two of 300 and one of 100.
    assert_eq!(
        lib.folders().unwrap().len(),
        4,
        "the root and three under it"
    );
    let cache = photon_core::thumbs::ThumbCache::new(&dirs.cache_dir);
    for entry in &entries {
        let item = lib.item(entry.id).unwrap().unwrap();
        assert_eq!(item.thumb_state, ThumbState::Ready);
        let thumbnail = cache.path_for(entry.thumb_key, photon_core::thumbs::ThumbSize::Grid);
        assert!(thumbnail.is_file(), "{}", thumbnail.display());
    }
    // The drive is unplugged: its folder is watched and is not there.
    let watched = lib.watched_folders().unwrap();
    assert_eq!(watched.len(), 1);
    assert!(!Path::new(&watched[0].path).exists());
    // And recorded as gone, as the engine's first scan would record it: left for that scan
    // to find, the first application to open the fixture rebuilds its grid for the change
    // and the second does not, and the two are compared.
    assert!(!watched[0].online);
    // And the cache needs no walk.
    assert_eq!(
        lib.thumb_gc_due(photon_core::now_ms(), Duration::from_secs(3600))
            .unwrap(),
        None
    );

    // A second build into the same place is refused before it writes anything, not
    // layered over the first: without the check it fails too, but half-way, on a folder
    // the library already watches, having made the folder again.
    let again = fixture::build(&dir.path().join("fixture"), 10, &sources(dir.path()));
    let refused = again.expect_err("a second build is refused").to_string();
    assert!(refused.contains("already holds a library"), "{refused}");
    assert!(!dir.path().join("fixture/unplugged-drive").exists());
}
