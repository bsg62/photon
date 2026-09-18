use crate::{Result, decode::decode_oriented, edit::Edit};
use image::DynamicImage;
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThumbSize {
    Grid,
    Preview,
}

impl ThumbSize {
    pub const ALL: [ThumbSize; 2] = [ThumbSize::Grid, ThumbSize::Preview];

    /// Longest edge in pixels.
    pub fn max_edge(self) -> u32 {
        match self {
            Self::Grid => 256,
            Self::Preview => 1600,
        }
    }

    fn dir_name(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::Preview => "preview",
        }
    }
}

const WEBP_QUALITY: f32 = 85.0;

/// libwebp's speed/size trade-off, 0 (fastest) to 6 (smallest). The crate's plain `encode`
/// uses 4, which cost ~70ms of the ~225ms it takes to thumbnail a 24 MP photo; 2 encodes
/// the same 1600px preview in ~19ms for files about 15% larger. Thumbnails are written once
/// and read from a local disk, so the bytes are cheap and the worker time is not: this is
/// roughly a fifth more import throughput per worker. Measured on a synthetic image; a real
/// photo's size difference may be smaller or larger, its time saving similar.
const WEBP_METHOD: i32 = 2;

/// Prefix for the temp file `write_webp` renames into place. Named by us rather than left to
/// `tempfile`'s default so garbage collection can recognise one.
const TEMP_PREFIX: &str = "thumb-";

/// How long an abandoned temp file must have sat untouched before GC reclaims it. Long
/// enough that a temp file a worker is still writing is never in scope, whatever the machine
/// is doing.
const TEMP_GRACE: Duration = Duration::from_secs(60 * 60);

/// On-disk WebP thumbnails keyed by content fingerprint.
pub struct ThumbCache {
    root: PathBuf,
}

impl ThumbCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn path_for(&self, fp: u64, size: ThumbSize) -> PathBuf {
        // The same spelling the UI sees as `thumbKey` and `collect_garbage` parses back, so
        // the directory layout, the wire format and the GC parser cannot drift apart.
        let hex = crate::grid::hex_key(fp);
        self.root
            .join(size.dir_name())
            .join(&hex[..2])
            .join(format!("{hex}.webp"))
    }

    pub fn is_complete(&self, fp: u64) -> bool {
        ThumbSize::ALL
            .iter()
            .all(|&size| self.path_for(fp, size).is_file())
    }

    /// Decodes `source` once and writes the preview and grid thumbnails of the untouched
    /// photo. The service renders through `render` with the item's edit; this is the plain
    /// form the cache's own tests use.
    pub fn generate(&self, source: &Path, orientation: u8, fp: u64) -> Result<()> {
        let (preview, grid) = self.render(source, orientation, Edit::default())?;
        self.store(fp, &preview, &grid)
    }

    /// Decodes `source` and produces the preview and grid images of the photo under `edit`,
    /// without touching disk. Failures here mean the source file itself is unreadable/corrupt.
    ///
    /// A crop is taken from the full-size decode and only then shrunk. Shrinking first, as
    /// the uncropped path does for speed, would crop an already small preview: a quarter of
    /// the frame would come out at half the preview's resolution, visibly soft in the
    /// viewer until the full image arrived. A turn costs nothing either way, so an edit
    /// without a crop keeps the fast path.
    pub(crate) fn render(
        &self,
        source: &Path,
        orientation: u8,
        edit: Edit,
    ) -> Result<(DynamicImage, DynamicImage)> {
        let preview_edge = ThumbSize::Preview.max_edge();
        let preview = if edit.crop.is_some() {
            let full = decode_oriented(source, orientation, u32::MAX)?;
            shrink(&edit.apply(full), preview_edge)
        } else {
            edit.apply(decode_oriented(source, orientation, preview_edge)?)
        };
        let grid = shrink(&preview, ThumbSize::Grid.max_edge());
        Ok((preview, grid))
    }

    /// Writes already-rendered thumbnails to the cache. Failures here mean the cache
    /// destination itself is unwritable (full disk, permissions), not that the source is bad.
    pub(crate) fn store(&self, fp: u64, preview: &DynamicImage, grid: &DynamicImage) -> Result<()> {
        write_webp(preview, &self.path_for(fp, ThumbSize::Preview))?;
        write_webp(grid, &self.path_for(fp, ThumbSize::Grid))?;
        Ok(())
    }

    /// Removes thumbnails whose fingerprint is not in `live`. Returns the number of files removed.
    ///
    /// GC is best-effort: an unreadable directory entry or a file that can't be removed
    /// (e.g. held open on Windows) is logged and skipped rather than aborting the walk.
    pub fn collect_garbage(&self, live: &HashSet<u64>) -> Result<usize> {
        let mut removed = 0;
        for entry in walkdir::WalkDir::new(&self.root).into_iter() {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    tracing::warn!(%err, "skipping unreadable cache entry");
                    continue;
                }
            };
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("webp") {
                if is_abandoned_temp(&entry) {
                    match fs::remove_file(path) {
                        Ok(()) => removed += 1,
                        Err(err) => {
                            tracing::warn!(%err, ?path, "could not remove a leaked temp file")
                        }
                    }
                }
                continue;
            }
            let Some(fp) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| u64::from_str_radix(s, 16).ok())
            else {
                continue;
            };
            if !live.contains(&fp) {
                match fs::remove_file(path) {
                    Ok(()) => removed += 1,
                    Err(err) => tracing::warn!(%err, ?path, "could not remove stale thumbnail"),
                }
            }
        }
        Ok(removed)
    }
}

/// Whether `entry` is a temp file left behind by a `write_webp` that never finished - a
/// process killed between `new_in` and `persist_noclobber`. Nothing else ever reclaims one,
/// so without this each leak is permanent.
///
/// Age is what separates a leak from a temp file a worker is writing right now. Anything
/// that cannot be aged (its metadata won't read) is left alone: removing a file another
/// thread is midway through writing would fail that thumbnail for nothing.
fn is_abandoned_temp(entry: &walkdir::DirEntry) -> bool {
    if !entry.file_type().is_file()
        || !entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(TEMP_PREFIX))
    {
        return false;
    }
    entry
        .metadata()
        .ok()
        .and_then(|md| md.modified().ok())
        .and_then(|at| at.elapsed().ok())
        .is_some_and(|age| age > TEMP_GRACE)
}

fn shrink(img: &DynamicImage, max_edge: u32) -> DynamicImage {
    if img.width().max(img.height()) > max_edge {
        img.thumbnail(max_edge, max_edge)
    } else {
        img.clone()
    }
}

/// Writes through a temp file + rename so readers never see a half-written thumbnail.
///
/// Paths are content-addressed, so an existing file already holds the same thumbnail:
/// it's kept rather than replaced, which also avoids failing on Windows when that file
/// is open.
fn write_webp(img: &DynamicImage, dest: &Path) -> Result<()> {
    // Encoded straight from RGB where there is no alpha to keep, which is every JPEG - the
    // overwhelming majority. `to_rgba8` allocates and copies a buffer a third larger than
    // the image for each of the two sizes written per photo, which on an import of any size
    // is the largest pointless allocation in the pool.
    let data = match img {
        DynamicImage::ImageRgb8(rgb) => encode_webp(&webp::Encoder::from_rgb(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
        ))?,
        _ => {
            let rgba = img.to_rgba8();
            encode_webp(&webp::Encoder::from_rgba(
                rgba.as_raw(),
                rgba.width(),
                rgba.height(),
            ))?
        }
    };
    let dir = dest.parent().expect("thumbnail path has a parent");
    fs::create_dir_all(dir)?;
    let mut tmp = tempfile::Builder::new()
        .prefix(TEMP_PREFIX)
        .tempfile_in(dir)?;
    tmp.write_all(&data)?;
    match tmp.persist_noclobber(dest) {
        Ok(_) => Ok(()),
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e.error.into()),
    }
}

/// Encodes at [`WEBP_QUALITY`] and [`WEBP_METHOD`]. A failure here is libwebp refusing its
/// own config or running out of memory, neither of which says anything about the source
/// photo, so it is reported as I/O: `process_item` then leaves the item `Pending` for a
/// retry rather than recording it as `Failed`.
fn encode_webp(encoder: &webp::Encoder<'_>) -> Result<webp::WebPMemory> {
    let mut config = webp::WebPConfig::new()
        .map_err(|()| std::io::Error::other("libwebp rejected its default config"))?;
    config.quality = WEBP_QUALITY;
    config.method = WEBP_METHOD;
    encoder
        .encode_advanced(&config)
        .map_err(|err| std::io::Error::other(format!("webp encoding failed: {err:?}")).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{jpeg_bytes, write_file};
    use std::time::Duration;

    fn dims(path: &Path) -> (u32, u32) {
        let img = image::open(path).unwrap();
        (img.width(), img.height())
    }

    #[test]
    fn paths_are_sharded_by_fingerprint() {
        let cache = ThumbCache::new("cache-root");
        assert_eq!(
            cache.path_for(0xabcd_ef01_2345_6789, ThumbSize::Grid),
            Path::new("cache-root")
                .join("grid")
                .join("ab")
                .join("abcdef0123456789.webp")
        );
        assert_eq!(
            cache.path_for(0x1, ThumbSize::Preview),
            Path::new("cache-root")
                .join("preview")
                .join("00")
                .join("0000000000000001.webp")
        );
    }

    #[test]
    fn generates_both_sizes_without_upscaling() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_file(dir.path(), "src.jpg", &jpeg_bytes(800, 400));
        let cache = ThumbCache::new(dir.path().join("cache"));
        assert!(!cache.is_complete(42));
        cache.generate(&src, 1, 42).unwrap();
        assert!(cache.is_complete(42));
        assert_eq!(dims(&cache.path_for(42, ThumbSize::Grid)), (256, 128));
        assert_eq!(dims(&cache.path_for(42, ThumbSize::Preview)), (800, 400));
    }

    #[test]
    fn thumbnails_are_oriented() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_file(dir.path(), "src.jpg", &jpeg_bytes(800, 400));
        let cache = ThumbCache::new(dir.path().join("cache"));
        cache.generate(&src, 6, 7).unwrap();
        assert_eq!(dims(&cache.path_for(7, ThumbSize::Grid)), (128, 256));
    }

    #[test]
    fn a_crop_is_taken_at_full_size_and_only_then_shrunk() {
        // The left half of a 3400-wide photo is 1700 wide: more than a preview holds, so
        // the preview must come out at its full 1600. Cropping the already shrunk 1600
        // preview instead gives 800 - the soft picture this order exists to avoid. A thin
        // strip, because a debug build decodes a square this wide in seconds.
        let dir = tempfile::tempdir().unwrap();
        let src = write_file(dir.path(), "src.jpg", &jpeg_bytes(3400, 170));
        let cache = ThumbCache::new(dir.path().join("cache"));
        let left_half = Edit::new(
            0,
            Some(crate::edit::Crop {
                left: 0,
                top: 0,
                right: (crate::edit::CROP_UNIT / 2) as u16,
                bottom: crate::edit::CROP_UNIT as u16,
            }),
        )
        .unwrap();
        let (preview, grid) = cache.render(&src, 1, left_half).unwrap();
        assert_eq!((preview.width(), preview.height()), (1600, 160));
        assert_eq!((grid.width(), grid.height()), (256, 26));
    }

    #[test]
    fn a_turn_is_applied_to_the_thumbnails() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_file(dir.path(), "src.jpg", &jpeg_bytes(800, 400));
        let cache = ThumbCache::new(dir.path().join("cache"));
        let (preview, grid) = cache.render(&src, 1, Edit::new(1, None).unwrap()).unwrap();
        assert_eq!((preview.width(), preview.height()), (400, 800));
        assert_eq!((grid.width(), grid.height()), (128, 256));
    }

    #[test]
    fn failed_generation_leaves_no_files() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_file(dir.path(), "bad.jpg", b"garbage");
        let cache = ThumbCache::new(dir.path().join("cache"));
        assert!(cache.generate(&src, 1, 9).is_err());
        assert!(!cache.is_complete(9));
        assert!(!cache.path_for(9, ThumbSize::Grid).exists());
    }

    #[test]
    fn regenerating_an_existing_thumbnail_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_file(dir.path(), "src.jpg", &jpeg_bytes(64, 64));
        let cache = ThumbCache::new(dir.path().join("cache"));
        cache.generate(&src, 1, 5).unwrap();
        // Keep a handle open, as a viewer would; Windows can't replace an open file.
        let _open = fs::File::open(cache.path_for(5, ThumbSize::Grid)).unwrap();
        cache.generate(&src, 1, 5).unwrap();
        assert!(cache.is_complete(5));
    }

    /// A process killed between `new_in` and `persist_noclobber` leaves its temp file
    /// behind, and GC only ever looked at `.webp` files - so one leaked file per worker
    /// accumulated over the life of an install with nothing able to reclaim it.
    #[test]
    fn garbage_collection_reclaims_abandoned_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_file(dir.path(), "src.jpg", &jpeg_bytes(64, 64));
        let cache = ThumbCache::new(dir.path().join("cache"));
        cache.generate(&src, 1, 5).unwrap();
        let shard = cache
            .path_for(5, ThumbSize::Grid)
            .parent()
            .unwrap()
            .to_path_buf();

        let abandoned = write_file(&shard, &format!("{TEMP_PREFIX}dead"), b"half a webp");
        let in_progress = write_file(&shard, &format!("{TEMP_PREFIX}live"), b"being written");
        let long_ago = std::time::SystemTime::now() - TEMP_GRACE - Duration::from_secs(60);
        fs::File::options()
            .write(true)
            .open(&abandoned)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();

        let removed = cache.collect_garbage(&HashSet::from([5])).unwrap();

        assert_eq!(removed, 1);
        assert!(!abandoned.exists(), "the leaked temp file is reclaimed");
        assert!(
            in_progress.exists(),
            "a temp file young enough to be a running worker's is left alone"
        );
        assert!(cache.is_complete(5), "and the live thumbnail is untouched");
    }

    #[test]
    #[cfg(unix)]
    fn garbage_collection_skips_files_it_cannot_remove() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let src = write_file(dir.path(), "src.jpg", &jpeg_bytes(64, 64));
        let cache = ThumbCache::new(dir.path().join("cache"));
        let (locked_fp, free_fp) = (0x0000_0000_0000_0001, 0xff00_0000_0000_0001);
        cache.generate(&src, 1, locked_fp).unwrap();
        cache.generate(&src, 1, free_fp).unwrap();
        let locked = cache.path_for(locked_fp, ThumbSize::Grid);
        let shard = locked.parent().unwrap();
        fs::set_permissions(shard, fs::Permissions::from_mode(0o555)).unwrap();
        if fs::remove_file(&locked).is_ok() {
            // Elevated privileges ignore permission bits; nothing to exercise here.
            fs::set_permissions(shard, fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }

        let removed = cache.collect_garbage(&HashSet::new());
        fs::set_permissions(shard, fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(removed.unwrap(), 3, "everything but the locked file");
        assert!(locked.is_file());
        assert!(!cache.path_for(free_fp, ThumbSize::Grid).exists());
    }

    #[test]
    fn garbage_collection_keeps_live_fingerprints() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_file(dir.path(), "src.jpg", &jpeg_bytes(64, 64));
        let cache = ThumbCache::new(dir.path().join("cache"));
        cache.generate(&src, 1, 1).unwrap();
        cache.generate(&src, 1, 2).unwrap();
        assert_eq!(cache.collect_garbage(&HashSet::from([1])).unwrap(), 2);
        assert!(cache.is_complete(1));
        assert!(!cache.is_complete(2));
    }
}
