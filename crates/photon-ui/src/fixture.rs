//! A library made to be measured: any number of photos, every one with its grid thumbnail
//! already in the cache, and nothing for the engine to do when it opens it.
//!
//! The gate runs two applications over the same library and compares them, so the library
//! has to be the same every time and has to keep the engine quiet. Three things do that:
//!
//! - **The watched folder does not exist.** It is made, watched, and removed again. To the
//!   engine that is an unplugged drive: its scan finds no root and stops, nothing is marked
//!   missing or purged, and since a folder is watched, no Pictures folder is added.
//! - **Every row is `Ready`.** Left `Pending`, the engine would queue every photo for a
//!   render at launch and fail each one, the files not being there, all through the
//!   measurement.
//! - **The cache is recorded as clean**, so the launch does not walk every file in it.
//!
//! The thumbnails are a few real pictures, each hard-linked under many keys: 300,000
//! thumbnails take the room of a few dozen. That makes them cheaper to read than a real
//! library's, for both applications alike, which the gate's write-up says.

use crate::dirs::{self, Dirs, IDENTIFIER};
use photon_core::{
    library::{Library, NewItem},
    media::{MediaKind, ThumbState},
    metadata::CameraMeta,
    now_ms,
    thumbs::{ThumbCache, ThumbSize},
};
use std::{
    collections::HashSet,
    error::Error,
    path::{Path, PathBuf},
    time::Duration,
};

/// Photos in each folder, and so under each header.
pub const PER_FOLDER: usize = 300;

/// Which builder this is. Moved whenever a fixture it builds would differ from the last
/// one's, so that the gate does not measure over a library made to other rules.
pub const BUILDER: u32 = 1;
/// The file a finished fixture has, in its directory.
pub const MARKER: &str = "fixture.json";

/// How many photos the fixture in `out` holds, if it was finished, and by this builder.
pub fn complete(out: &Path) -> Option<usize> {
    let said = std::fs::read(out.join(MARKER)).ok()?;
    let said: serde_json::Value = serde_json::from_slice(&said).ok()?;
    (said["builder"] == BUILDER).then_some(())?;
    Some(said["photos"].as_u64()? as usize)
}

/// Where a fixture is, in both spellings: the directories photon-native is given, and the
/// two homes the Tauri photon is pointed at (`XDG_DATA_HOME`, `XDG_CACHE_HOME`), under
/// which it resolves the same directories by its identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixture {
    pub data_home: PathBuf,
    pub cache_home: PathBuf,
    /// `<data_home>/<identifier>`: what `--data-dir` names.
    pub data_dir: PathBuf,
    /// `<cache_home>/<identifier>`: what `--cache-dir` names.
    pub cache_dir: PathBuf,
    pub photos: usize,
}

impl Fixture {
    pub fn at(out: &Path, photos: usize) -> Self {
        let (data_home, cache_home) = (out.join("data"), out.join("cache"));
        Self {
            data_dir: data_home.join(IDENTIFIER),
            cache_dir: cache_home.join(IDENTIFIER),
            data_home,
            cache_home,
            photos,
        }
    }

    pub fn dirs(&self) -> Dirs {
        dirs::within(&self.data_dir, &self.cache_dir)
    }
}

/// Builds a library of `photos` photos under `out`, their thumbnails made from the
/// pictures in `sources`. `out` must not hold a fixture already.
pub fn build(out: &Path, photos: usize, sources: &[PathBuf]) -> Result<Fixture, Box<dyn Error>> {
    if sources.is_empty() {
        return Err("a fixture needs at least one picture to make thumbnails from".into());
    }
    let fixture = Fixture::at(out, photos);
    let dirs = fixture.dirs();
    if dirs.db_path.exists() {
        return Err(format!("{} already holds a library", out.display()).into());
    }
    std::fs::create_dir_all(&fixture.data_dir)?;

    // The folder is real for as long as the library needs it to be: a folder that is not
    // there cannot be watched.
    let root = out.join("unplugged-drive");
    std::fs::create_dir_all(&root)?;
    let lib = Library::open(&dirs.db_path)?;
    let watched = lib.add_watched_folder(&root, &[])?;
    let root_path = watched.path.clone();
    let root_id = lib.upsert_folder(watched.id, None, &root_path, 1)?;

    let mut items = Vec::with_capacity(photos);
    for folder in 0..photos.div_ceil(PER_FOLDER) {
        let folder_path = format!("{root_path}/folder-{folder:04}");
        let folder_id = lib.upsert_folder(watched.id, Some(root_id), &folder_path, 1)?;
        let first = folder * PER_FOLDER;
        for n in first..photos.min(first + PER_FOLDER) {
            let file_name = format!("IMG_{n:06}.jpg");
            // Landscape, portrait and wide, so tiles crop as a real library's do.
            let (width, height) = [(4000, 3000), (3000, 4000), (6000, 4000)][n % 3];
            items.push(NewItem {
                folder_id,
                path: format!("{folder_path}/{file_name}"),
                file_name,
                kind: MediaKind::Image,
                size: 2_000_000 + (n as i64 % 4_000) * 1_000,
                mtime_ms: 1_600_000_000_000 + n as i64,
                width,
                height,
                orientation: 1,
                // A folder every three days from 2017 on, a photo a minute within it.
                taken_at: 1_500_000_000 + folder as i64 * 259_200 + (n - first) as i64 * 60,
                rating: None,
                camera: CameraMeta::default(),
                tags: Vec::new(),
                caption: None,
                duration_ms: None,
            });
        }
    }
    for batch in items.chunks(10_000) {
        lib.insert_items(batch)?;
    }

    // A few real thumbnails, made the way the engine makes them, under keys of their own.
    let made = ThumbCache::new(out.join("made-thumbnails"));
    let mut pictures = Vec::with_capacity(sources.len());
    for (n, source) in sources.iter().enumerate() {
        made.generate(source, 1, n as u64)
            .map_err(|err| format!("{}: {err}", source.display()))?;
        pictures.push(made.path_for(n as u64, ThumbSize::Grid));
    }

    // Each photo's grid thumbnail is one of them, under the photo's own key.
    let cache = ThumbCache::new(&dirs.cache_dir);
    let entries = lib.grid_entries()?;
    let mut shards = HashSet::new();
    for (n, entry) in entries.iter().enumerate() {
        let path = cache.path_for(entry.thumb_key, ThumbSize::Grid);
        let shard = path.parent().expect("a thumbnail is in a directory");
        if shards.insert(shard.to_path_buf()) {
            std::fs::create_dir_all(shard)?;
        }
        let picture = &pictures[n % pictures.len()];
        // A link where the filesystem has room for one more to this file; a copy where it
        // has not, or `out` spans two filesystems.
        if std::fs::hard_link(picture, &path).is_err() {
            std::fs::copy(picture, &path)?;
        }
        lib.set_thumb_state(entry.id, ThumbState::Ready, None)?;
    }

    // Nothing in the cache is garbage, and the library says so: no walk of it at launch.
    let now = now_ms();
    if let Some(epoch) = lib.thumb_gc_due(now, Duration::from_secs(1))? {
        lib.thumb_gc_done(epoch, now)?;
    }
    // The drive is gone and the library knows: found out by the first scan instead, that
    // is a grid rebuilt in whichever application opens the fixture first.
    lib.set_watched_online(watched.id, false)?;
    drop(lib);
    std::fs::remove_dir_all(&root)?;
    // Last: a build that stopped anywhere above left a library, and no word that it is
    // one to measure over.
    let said = serde_json::json!({ "builder": BUILDER, "photos": photos });
    std::fs::write(out.join(MARKER), said.to_string())?;
    Ok(fixture)
}
