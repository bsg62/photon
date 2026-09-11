use crate::{Result, decode::decode_oriented};
use image::DynamicImage;
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
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

/// On-disk WebP thumbnails keyed by content fingerprint.
pub struct ThumbCache {
    root: PathBuf,
}

impl ThumbCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn path_for(&self, fp: u64, size: ThumbSize) -> PathBuf {
        let hex = format!("{fp:016x}");
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

    /// Decodes `source` once and writes the preview and grid thumbnails.
    pub fn generate(&self, source: &Path, orientation: u8, fp: u64) -> Result<()> {
        let (preview, grid) = self.render(source, orientation)?;
        self.store(fp, &preview, &grid)
    }

    /// Decodes `source` and produces the preview and grid images, without touching disk.
    /// Failures here mean the source file itself is unreadable/corrupt.
    pub(crate) fn render(
        &self,
        source: &Path,
        orientation: u8,
    ) -> Result<(DynamicImage, DynamicImage)> {
        let preview = decode_oriented(source, orientation, ThumbSize::Preview.max_edge())?;
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
    /// GC is best-effort: an unreadable directory entry is logged and skipped rather than
    /// aborting the whole walk.
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
                fs::remove_file(path)?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

fn shrink(img: &DynamicImage, max_edge: u32) -> DynamicImage {
    if img.width().max(img.height()) > max_edge {
        img.thumbnail(max_edge, max_edge)
    } else {
        img.clone()
    }
}

/// Writes through a temp file + rename so readers never see a half-written thumbnail.
fn write_webp(img: &DynamicImage, dest: &Path) -> Result<()> {
    let rgba = img.to_rgba8();
    let data =
        webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height()).encode(WEBP_QUALITY);
    let dir = dest.parent().expect("thumbnail path has a parent");
    fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(&data)?;
    tmp.persist(dest).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{jpeg_bytes, write_file};

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
    fn failed_generation_leaves_no_files() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_file(dir.path(), "bad.jpg", b"garbage");
        let cache = ThumbCache::new(dir.path().join("cache"));
        assert!(cache.generate(&src, 1, 9).is_err());
        assert!(!cache.is_complete(9));
        assert!(!cache.path_for(9, ThumbSize::Grid).exists());
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
