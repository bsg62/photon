use crate::{
    Result,
    decode::{decode_oriented, fit_within},
    edit::{Edit, render_picture},
};
use image::{DynamicImage, RgbImage};
use std::{
    collections::{HashSet, VecDeque},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
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

    /// libwebp's speed/size trade-off for this size, 0 (fastest) to 6 (smallest). The
    /// crate's plain `encode` uses 4, which cost ~70 ms of the ~225 ms a 24 MP photo's
    /// thumbnail took on Apple Silicon; 2 was chosen for both sizes on that measurement.
    ///
    /// The preview is at 1. On x86-64 its encode was still ~40 ms at 2, as long as the scaled
    /// JPEG decode before it and nearly half a smooth 24 MP photo's thumbnail; 1 takes ~30 ms.
    /// Measured on a 1600 px preview of a real photo upscaled to 24 MP, and of the render
    /// bench's noise: 29.8 against 39.9 ms for 16% more bytes (183 against 158 KB), and 22.1
    /// against 38.2 ms for 14% fewer; the decoded pictures' PSNR within 0.6 dB either way
    /// (40.3 against 39.8, 38.2 against 38.3). Thumbnails are written once and read from a
    /// local disk, so the bytes are cheap and the worker time is not.
    ///
    /// The grid thumbnail stays at 2. Its encode is ~1.5 ms either way, and it is the picture
    /// the look-alike pass hashes and compares, which is better left as every cached grid
    /// thumbnail already has it.
    fn webp_method(self) -> i32 {
        match self {
            Self::Grid => 2,
            Self::Preview => 1,
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

/// Prefix for the temp file `write_webp` renames into place. Named by us rather than left to
/// `tempfile`'s default so garbage collection can recognise one.
const TEMP_PREFIX: &str = "thumb-";

/// How long an abandoned temp file must have sat untouched before GC reclaims it. Long
/// enough that a temp file a worker is still writing is never in scope, whatever the machine
/// is doing.
const TEMP_GRACE: Duration = Duration::from_secs(60 * 60);

/// How many decoded previews `face_crop` keeps, the most recently used. A group photo's
/// faces are crops of one preview, asked for together as a page of strips loads; kept, it is
/// decoded once for all of them rather than once a face. Eight 1600 px previews are 46 MB
/// at 4:3 and 61 MB square, held from the first crop on.
const DECODED_PREVIEWS: usize = 8;

/// On-disk WebP thumbnails keyed by content fingerprint.
pub struct ThumbCache {
    root: PathBuf,
    /// The last [`DECODED_PREVIEWS`] previews `face_crop` decoded, by key, the most recently
    /// used last. A key names one picture, so a decode kept under it is never stale.
    decoded: parking_lot::Mutex<VecDeque<(u64, Arc<RgbImage>)>>,
}

impl ThumbCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            decoded: parking_lot::Mutex::new(VecDeque::with_capacity(DECODED_PREVIEWS + 1)),
        }
    }

    /// The cache directory, for the in-flight markers kept beside the thumbnails.
    pub(crate) fn root(&self) -> &Path {
        &self.root
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

    /// Moves each size's cached file from key `from` to key `to`. A key is made of the
    /// file's path, so a photo that was renamed or moved names thumbnails that are not
    /// cached, while the ones under its old key are of the very same picture: it has not
    /// changed, only what it is called. A size with nothing under `from` is skipped. A rename
    /// that fails is logged and costs one render, never a wrong picture, since a key names
    /// one picture.
    pub fn rename(&self, from: u64, to: u64) {
        for size in ThumbSize::ALL {
            let (src, dst) = (self.path_for(from, size), self.path_for(to, size));
            if !src.is_file() {
                continue;
            }
            let moved = match dst.parent() {
                Some(dir) => fs::create_dir_all(dir),
                None => Ok(()),
            }
            .and_then(|()| fs::rename(&src, &dst));
            if let Err(err) = moved {
                tracing::warn!(%err, from, to, ?size, "could not carry a thumbnail to its new key");
            }
        }
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
            // The viewer's own full-size render, which crops before it turns anything.
            fit_within(render_picture(source, orientation, edit)?, preview_edge)
        } else {
            edit.apply(decode_oriented(source, orientation, preview_edge)?)
        };
        let grid = shrink(&preview, ThumbSize::Grid.max_edge());
        Ok((preview, grid))
    }

    /// A poster frame the webview drew, as the preview and grid images. It arrives upright
    /// and at most preview-sized, so this only shrinks.
    pub(crate) fn render_frame(&self, frame: &DynamicImage) -> (DynamicImage, DynamicImage) {
        let preview = shrink(frame, ThumbSize::Preview.max_edge());
        let grid = shrink(&preview, ThumbSize::Grid.max_edge());
        (preview, grid)
    }

    /// Writes already-rendered thumbnails to the cache. Failures here mean the cache
    /// destination itself is unwritable (full disk, permissions), not that the source is bad.
    pub(crate) fn store(&self, fp: u64, preview: &DynamicImage, grid: &DynamicImage) -> Result<()> {
        for (img, size) in [(preview, ThumbSize::Preview), (grid, ThumbSize::Grid)] {
            write_webp(img, &self.path_for(fp, size), size.webp_method())?;
        }
        Ok(())
    }

    /// Decodes a thumbnail file with libwebp, the library that wrote it, rather than with
    /// `image`'s pure-Rust `image-webp`. The pixels are the same - the look-alike pass stored
    /// every `percep_hash` before this from `image-webp`'s decode, and a new hash has to be
    /// comparable with those (`reading_gives_image_webps_pixels` holds it) - and the decode
    /// is about three times faster: ~0.10 ms against ~0.32 for a grid thumbnail. That pass
    /// reads every grid thumbnail in the library on its first run over it, and the native
    /// grid reads one for every tile it draws.
    pub fn decode(path: &Path) -> Result<DynamicImage> {
        let bytes = fs::read(path)?;
        // `None` is libwebp refusing the file, or an animation, which photon never writes.
        webp::Decoder::new(&bytes)
            .decode()
            .map(|img| img.to_image())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "not a still WebP").into()
            })
    }

    /// The thumbnail cached for `fp` at `size`, decoded; see `decode`.
    pub(crate) fn read(&self, fp: u64, size: ThumbSize) -> Result<DynamicImage> {
        Self::decode(&self.path_for(fp, size))
    }

    /// A face's crop, as WebP: the square `face_crop::square` gives, cut from the cached
    /// preview and scaled to `CROP_PX`. Only the cache is read - never the photo, and never a
    /// render: a preview that is not cached is an I/O `NotFound`, which the route answers with
    /// a 404 and the page with a placeholder. The decoded preview is kept for the photo's
    /// other faces (`decoded_preview`).
    pub fn face_crop(&self, key: u64, rect: &crate::face_detect::Rect) -> Result<Vec<u8>> {
        use super::face_crop::{CROP_PX, square};
        let preview = self.decoded_preview(key)?;
        let (x, y, side) = square(rect, preview.width(), preview.height()).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "no face to crop")
        })?;
        let cut = image::imageops::crop_imm(&*preview, x, y, side, side).to_image();
        let small = image::imageops::resize(
            &cut,
            CROP_PX,
            CROP_PX,
            image::imageops::FilterType::Triangle,
        );
        let encoded = encode_webp(
            &webp::Encoder::from_rgb(small.as_raw(), CROP_PX, CROP_PX),
            ThumbSize::Grid.webp_method(),
        )?;
        Ok(encoded.to_vec())
    }

    /// The cached preview under `key`, decoded: from the ones kept if it is there, else read
    /// from disk and kept. Decoded outside the lock, so two crops of different photos decode
    /// side by side; two of the same photo arriving together may both decode it, which costs
    /// a decode and no more.
    fn decoded_preview(&self, key: u64) -> Result<Arc<RgbImage>> {
        {
            let mut kept = self.decoded.lock();
            if let Some(at) = kept.iter().position(|(k, _)| *k == key) {
                let hit = kept.remove(at).expect("a position just found");
                let image = hit.1.clone();
                kept.push_back(hit);
                return Ok(image);
            }
        }
        let image = Arc::new(self.read(key, ThumbSize::Preview)?.to_rgb8());
        let mut kept = self.decoded.lock();
        if !kept.iter().any(|(k, _)| *k == key) {
            kept.push_back((key, image.clone()));
            if kept.len() > DECODED_PREVIEWS {
                kept.pop_front();
            }
        }
        Ok(image)
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
    crate::decode::shrink_within(img, max_edge)
}

/// Writes through a temp file + rename so readers never see a half-written thumbnail.
///
/// Paths are content-addressed, so an existing file already holds the same thumbnail:
/// it's kept rather than replaced, which also avoids failing on Windows when that file
/// is open.
fn write_webp(img: &DynamicImage, dest: &Path, method: i32) -> Result<()> {
    // Encoded straight from RGB where there is no alpha to keep, which is every JPEG - the
    // overwhelming majority. `to_rgba8` allocates and copies a buffer a third larger than
    // the image for each of the two sizes written per photo, which on an import of any size
    // is the largest pointless allocation in the pool.
    let data = match img {
        DynamicImage::ImageRgb8(rgb) => encode_webp(
            &webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height()),
            method,
        )?,
        _ => {
            let rgba = img.to_rgba8();
            encode_webp(
                &webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height()),
                method,
            )?
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

/// Encodes at [`WEBP_QUALITY`] and the size's `method` ([`ThumbSize::webp_method`]). A failure here is libwebp refusing its
/// own config or running out of memory, neither of which says anything about the source
/// photo, so it is reported as I/O: `process_item` then leaves the item `Pending` for a
/// retry rather than recording it as `Failed`.
fn encode_webp(encoder: &webp::Encoder<'_>, method: i32) -> Result<webp::WebPMemory> {
    let mut config = webp::WebPConfig::new()
        .map_err(|()| std::io::Error::other("libwebp rejected its default config"))?;
    config.quality = WEBP_QUALITY;
    config.method = method;
    encoder
        .encode_advanced(&config)
        .map_err(|err| std::io::Error::other(format!("webp encoding failed: {err:?}")).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{jpeg_bytes, write_file};
    use image::ImageFormat;
    use std::time::Duration;

    fn dims(path: &Path) -> (u32, u32) {
        let img = image::open(path).unwrap();
        (img.width(), img.height())
    }

    /// The grid thumbnail is shrunk from the preview by `fast_image_resize`, where it was
    /// `image`'s `thumbnail`. A cache holds grid thumbnails made both ways, and the
    /// look-alike pass hashes and compares them, so the two must be the same picture as far as
    /// `same_picture` can tell: the bar `decode.rs` holds the preview's resampler to. The
    /// stripes are near the sampling limit, where a filter that does not average aliases.
    #[test]
    fn a_grid_thumbnail_is_the_same_picture_images_thumbnail_made() {
        let (w, h) = (1600, 800);
        let preview = DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, y| {
            let wave = (x as f64 * 0.7975 * std::f64::consts::TAU).sin();
            let stripe = (127.0 + 100.0 * wave) as u8;
            let block = if (400..700).contains(&x) && (200..500).contains(&y) {
                40
            } else {
                (x * 255 / w) as u8
            };
            image::Rgb([stripe, block, (y * 255 / h) as u8])
        }));
        let ours = shrink(&preview, ThumbSize::Grid.max_edge());
        let theirs = preview.thumbnail(256, 256);
        assert_eq!(
            (ours.width(), ours.height(), ours.color()),
            (theirs.width(), theirs.height(), theirs.color())
        );
        let difference = crate::similar::picture_difference(
            &crate::similar::reduce(&ours),
            &crate::similar::reduce(&theirs),
        );
        assert!(
            difference < crate::similar::SAME_PICTURE_MAX_DIFFERENCE / 4.0,
            "{difference}"
        );
    }

    /// The preview is encoded at method 1 and the grid thumbnail at method 2 (see
    /// [`ThumbSize::webp_method`]). libwebp's encode is deterministic, so each file must be,
    /// byte for byte, its picture encoded at that method, and a size moved to the other
    /// method writes different bytes. Noise, so every block has detail for the methods'
    /// searches to differ on.
    #[test]
    fn each_size_is_encoded_at_its_own_method() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_file(
            dir.path(),
            "src.jpg",
            &crate::testutil::noisy_jpeg(1800, 300),
        );
        let cache = ThumbCache::new(dir.path().join("cache"));
        let (preview, grid) = cache.render(&src, 1, Edit::default()).unwrap();
        cache.store(7, &preview, &grid).unwrap();
        let encoded = |img: &DynamicImage, method: i32| {
            let rgb = img.to_rgb8();
            let mut config = webp::WebPConfig::new().unwrap();
            config.quality = WEBP_QUALITY;
            config.method = method;
            webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height())
                .encode_advanced(&config)
                .unwrap()
                .to_vec()
        };
        for (size, img, method, other) in [
            (ThumbSize::Preview, &preview, 1, 2),
            (ThumbSize::Grid, &grid, 2, 1),
        ] {
            let written = fs::read(cache.path_for(7, size)).unwrap();
            assert_eq!(written, encoded(img, method), "{size:?}");
            assert_ne!(
                written,
                encoded(img, other),
                "{size:?}: the methods agree here"
            );
        }
    }

    /// A 400 x 200 grey preview with a red 40 x 40 square at x 180-220, y 80-120.
    fn red_square_preview() -> DynamicImage {
        DynamicImage::ImageRgb8(image::RgbImage::from_fn(400, 200, |x, y| {
            if (180..220).contains(&x) && (80..120).contains(&y) {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([128, 128, 128])
            }
        }))
    }

    #[test]
    fn a_face_crop_is_cut_around_the_face_from_the_cached_preview() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ThumbCache::new(dir.path().join("cache"));
        let preview = red_square_preview();
        cache.store(5, &preview, &preview).unwrap();
        let rect = crate::face_detect::Rect {
            left: 0.45,
            top: 0.4,
            right: 0.55,
            bottom: 0.6,
        };
        let bytes = cache.face_crop(5, &rect).unwrap();
        let crop = webp::Decoder::new(&bytes).decode().unwrap().to_image();
        assert_eq!((crop.width(), crop.height()), (144, 144));
        let crop = crop.to_rgb8();
        let centre = crop.get_pixel(72, 72).0;
        assert!(
            centre[0] > 180 && centre[1] < 80 && centre[2] < 80,
            "{centre:?}"
        );
        let corner = crop.get_pixel(1, 1).0;
        assert!(
            corner.iter().all(|&c| (110..150).contains(&c)),
            "{corner:?}"
        );
    }

    /// The route answers exactly this kind with a 404.
    #[test]
    fn a_face_crop_is_cut_only_from_the_cached_preview() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ThumbCache::new(dir.path().join("cache"));
        let rect = crate::face_detect::Rect {
            left: 0.4,
            top: 0.4,
            right: 0.6,
            bottom: 0.6,
        };
        match cache.face_crop(9, &rect) {
            Err(crate::Error::Io(err)) => assert_eq!(err.kind(), std::io::ErrorKind::NotFound),
            other => panic!("{other:?}"),
        }
    }

    /// A photo's faces decode its preview once: a second face of the same key is cut from
    /// the decode kept from the first, even with the file gone, until eight other previews
    /// have been decoded since its last use.
    #[test]
    fn a_face_crop_reuses_the_last_eight_decoded_previews() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ThumbCache::new(dir.path().join("cache"));
        let preview = red_square_preview();
        for key in 1..=9 {
            cache.store(key, &preview, &preview).unwrap();
        }
        let face = |left: f64| crate::face_detect::Rect {
            left,
            top: 0.4,
            right: left + 0.1,
            bottom: 0.6,
        };
        cache.face_crop(1, &face(0.45)).unwrap();
        fs::remove_file(cache.path_for(1, ThumbSize::Preview)).unwrap();
        cache
            .face_crop(1, &face(0.1))
            .expect("the decode kept from the first face");
        for key in 2..=8 {
            cache.face_crop(key, &face(0.45)).unwrap();
        }
        // Key 1 is the least recently used of eight now: still kept.
        cache.face_crop(1, &face(0.45)).unwrap();
        cache.face_crop(9, &face(0.45)).unwrap();
        cache.face_crop(1, &face(0.45)).unwrap();
        // Key 2 was the oldest when 9 came in, and went for it.
        fs::remove_file(cache.path_for(2, ThumbSize::Preview)).unwrap();
        match cache.face_crop(2, &face(0.45)) {
            Err(crate::Error::Io(err)) => assert_eq!(err.kind(), std::io::ErrorKind::NotFound),
            other => panic!("{other:?}"),
        }
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
    fn rename_carries_both_sizes_to_the_new_key() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_file(dir.path(), "src.jpg", &jpeg_bytes(800, 400));
        let cache = ThumbCache::new(dir.path().join("cache"));
        cache.generate(&src, 1, 1).unwrap();
        let grid = fs::read(cache.path_for(1, ThumbSize::Grid)).unwrap();
        cache.rename(1, 2);
        assert!(cache.is_complete(2));
        for size in ThumbSize::ALL {
            assert!(!cache.path_for(1, size).exists(), "{size:?} left behind");
        }
        assert_eq!(fs::read(cache.path_for(2, ThumbSize::Grid)).unwrap(), grid);
    }

    #[test]
    fn rename_of_a_key_with_nothing_cached_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ThumbCache::new(dir.path().join("cache"));
        cache.rename(7, 8);
        assert!(!cache.is_complete(8));
        for size in ThumbSize::ALL {
            assert!(!cache.path_for(8, size).parent().unwrap().exists());
        }
    }

    #[test]
    fn rename_carries_a_lone_size() {
        let dir = tempfile::tempdir().unwrap();
        let src = write_file(dir.path(), "src.jpg", &jpeg_bytes(800, 400));
        let cache = ThumbCache::new(dir.path().join("cache"));
        cache.generate(&src, 1, 1).unwrap();
        fs::remove_file(cache.path_for(1, ThumbSize::Preview)).unwrap();
        // A key in another shard directory, which a path edit usually lands in: the
        // directory is not there to receive the file.
        let to = 0xab << 56;
        cache.rename(1, to);
        assert!(cache.path_for(to, ThumbSize::Grid).is_file());
        assert!(!cache.path_for(1, ThumbSize::Grid).exists());
        assert!(!cache.path_for(to, ThumbSize::Preview).exists());
    }

    /// `read` swapped `image-webp` for libwebp under hashes already stored, so the two have
    /// to agree to the byte, not merely look alike. The fixtures are where two decoders part
    /// if they part anywhere: noise, so every block carries detail; odd dimensions, whose
    /// last chroma sample covers a single pixel; a thumbnail kept at its source size and one
    /// shrunk; and an alpha channel, which `write_webp` encodes on a path of its own.
    #[test]
    fn reading_gives_image_webps_pixels() {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut noise = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed & 0x3F) as u8
        };
        let rgb = |w: u32, h: u32, noise: &mut dyn FnMut() -> u8| {
            DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, y| {
                let n = noise();
                image::Rgb([
                    (x * 190 / w) as u8 + n,
                    (y * 190 / h) as u8 + n,
                    ((x + y) * 90 / (w + h)) as u8 + n,
                ])
            }))
        };
        let rgba = DynamicImage::ImageRgba8(image::RgbaImage::from_fn(301, 199, |x, y| {
            let n = noise();
            image::Rgba([
                (x % 256) as u8,
                (y % 256) as u8,
                n * 3,
                (x + y) as u8 | 0x0F,
            ])
        }));
        let fixtures = [
            ("wide.jpg", rgb(800, 400, &mut noise), ImageFormat::Jpeg),
            ("odd.jpg", rgb(333, 517, &mut noise), ImageFormat::Jpeg),
            ("shrunk.jpg", rgb(1901, 1001, &mut noise), ImageFormat::Jpeg),
            ("alpha.png", rgba, ImageFormat::Png),
        ];

        let dir = tempfile::tempdir().unwrap();
        let cache = ThumbCache::new(dir.path().join("cache"));
        for (fp, (name, img, format)) in (1u64..).zip(&fixtures) {
            let src = write_file(dir.path(), name, &crate::testutil::encode(img, *format));
            cache.generate(&src, 1, fp).unwrap();
            for size in ThumbSize::ALL {
                let ours = cache.read(fp, size).unwrap();
                let theirs = image::open(cache.path_for(fp, size)).unwrap();
                // The alpha fixture has to reach `from_rgba`, or it proves nothing about it.
                assert_eq!(
                    theirs.color().has_alpha(),
                    *format == ImageFormat::Png,
                    "{name}"
                );
                assert_eq!(ours.color(), theirs.color(), "{name} {size:?}");
                let dims = |img: &DynamicImage| (img.width(), img.height());
                assert_eq!(dims(&ours), dims(&theirs), "{name} {size:?}");
                assert!(ours.as_bytes() == theirs.as_bytes(), "{name} {size:?}");
            }
        }
    }

    #[test]
    fn reading_a_missing_or_foreign_thumbnail_fails() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ThumbCache::new(dir.path().join("cache"));
        assert!(cache.read(3, ThumbSize::Grid).is_err());
        let path = cache.path_for(3, ThumbSize::Grid);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, jpeg_bytes(16, 16)).unwrap();
        assert!(cache.read(3, ThumbSize::Grid).is_err());
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
