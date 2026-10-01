//! Copying photos out of the library.
//!
//! This is the one place photon writes a photo file, and it writes only *new* files, at a
//! destination the user picked. Nothing here opens a watched photo for writing: a source is
//! read, and either its bytes are copied or a fresh picture is encoded beside them.
//!
//! The caller is responsible for refusing a destination inside a watched folder
//! (`Engine::export_items`); this module would happily write there, and the scanner would
//! then index the copies as new photos.

use crate::edit::{Chroma, Edit, encode_picture, render_picture};
use crate::error::Result;
use crate::media::MediaKind;
use crate::metadata::oriented_dims;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

/// A photo to copy out: where it is, how its EXIF says it is turned, the edit the user has
/// put on it, and what the library knows of its size and kind.
#[derive(Clone, Debug)]
pub struct Source {
    pub path: PathBuf,
    pub orientation: u8,
    pub edit: Edit,
    /// The stored pixel size, before `orientation`, as the library holds it; 0 when the
    /// header could not be read.
    pub width: u32,
    pub height: u32,
    pub kind: MediaKind,
}

/// What the user asked of an export.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Whether an edited photo comes out as it is shown, or as the file it was made from.
    pub apply_edits: bool,
    /// The longest edge a copy may have, in pixels; `None` for full size. A photo already
    /// within it is not touched by it.
    pub max_edge: Option<u32>,
}

/// How one photo is exported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    /// The file's own bytes.
    Copy,
    /// A fresh picture: decoded, given `edit`, and scaled down to `max_edge` when that is
    /// set.
    Render { edit: Edit, max_edge: Option<u32> },
}

/// High enough that the copy is worth keeping. The viewer's own render uses a lower quality
/// on purpose - it is thrown away after one look - and this is the case its doc comment says
/// it is not for.
pub const EXPORT_QUALITY: u8 = 95;

/// The quality of a copy scaled down. Someone who asks for 1920 pixels wants a file to send,
/// not an archive: at 90 with 4:2:0 colour it is about a third the bytes of
/// [`EXPORT_QUALITY`] at 4:4:4, and the scaling has already cost more detail than the
/// encoder does.
pub const RESIZED_QUALITY: u8 = 90;

impl Source {
    /// How this photo is exported under `options`.
    ///
    /// It is copied byte for byte unless something asks for a different picture: an edit to
    /// apply, or a size the photo exceeds. So an untouched photo within the size keeps every
    /// byte, EXIF included, whatever the options say.
    ///
    /// The size is judged from the library's stored dimensions, not by decoding: an export
    /// of a thousand photos that are all small enough must stay a thousand file copies. A
    /// photo whose header could not be read has no stored size and is copied. A video is
    /// always copied, and so is a GIF, whose frames a scaled copy would lose.
    pub fn plan(&self, options: Options) -> Plan {
        let edit = if options.apply_edits {
            self.edit
        } else {
            Edit::default()
        };
        let (w, h) = oriented_dims(self.width, self.height, self.orientation);
        let (w, h) = edit.dims(w, h);
        let scalable = self.kind == MediaKind::Image && !self.is_gif();
        let max_edge = options
            .max_edge
            .filter(|&max| max > 0 && scalable && w.max(h) > max);
        if edit.is_identity() && max_edge.is_none() {
            Plan::Copy
        } else {
            Plan::Render { edit, max_edge }
        }
    }

    fn is_gif(&self) -> bool {
        self.path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("gif"))
    }
}

/// The picture a [`Plan::Render`] asks for: decoded, turned upright, given `edit`, scaled
/// down to `max_edge` when that is set, and encoded.
///
/// Full size it is encoded as a copy to keep ([`EXPORT_QUALITY`], full colour); scaled down,
/// as one to send ([`RESIZED_QUALITY`]). Lanczos, as the clipboard's copy is: made once, to
/// be looked at.
///
/// Separate from the write because the caller holds `protocol::RENDERING` across *this* and
/// not across the write: the lock exists to bound how many full-size decodes are in memory
/// at once, and by the time the bytes are on their way to a slow USB stick the picture has
/// already been dropped.
pub fn render_for_export(
    src: &Source,
    edit: Edit,
    max_edge: Option<u32>,
) -> Result<(Vec<u8>, &'static str)> {
    let img = render_picture(&src.path, src.orientation, edit)?;
    match max_edge {
        None => encode_picture(img, EXPORT_QUALITY, Chroma::Full),
        Some(max) => encode_picture(
            crate::decode::fit_within_by(img, max, fast_image_resize::FilterType::Lanczos3),
            RESIZED_QUALITY,
            Chroma::Half,
        ),
    }
}

/// Writes an already rendered picture into `dir`, returning the path written.
///
/// The edited picture is no longer the source's format - a source with transparency is
/// re-encoded as PNG and anything else comes out JPEG - so the extension follows the bytes,
/// not the original name.
pub fn write_rendered(src: &Source, dir: &Path, mime: &str, bytes: &[u8]) -> Result<PathBuf> {
    let ext = if mime == "image/png" { "png" } else { "jpg" };
    create_new_in(dir, &stem(&src.path), ext, |file| file.write_all(bytes))
}

/// Copies a photo's own file into `dir`, returning the path written. Every byte, including
/// the EXIF photon does not write, since nothing is decoded on this path.
pub fn copy_original(src: &Source, dir: &Path) -> Result<PathBuf> {
    let ext = src
        .path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut source = std::fs::File::open(&src.path)?;
    create_new_in(dir, &stem(&src.path), &ext, |file| {
        std::io::copy(&mut source, file).map(|_| ())
    })
}

/// Creates `dir/stem.ext` - or `dir/stem (2).ext` and onwards when that name is taken - and
/// hands the new file to `write`. Returns the path written.
///
/// **`create_new` is what makes "nothing is ever overwritten" true**, rather than merely
/// intended. Looking with `exists()` and then writing is two things:
///
/// - a race. An export runs for minutes, and a sync client or a second photon window
///   creating that name in between would lose its file to the write that followed.
/// - a symlink. `exists()` follows one, so a *dangling* link in the destination
///   (`dest/IMG_1234.JPG -> /watched/photos/IMG_1234.JPG`, target not yet there) answers
///   "no" and the write then follows it - landing a new file inside a watched folder, which
///   is the one thing this feature must never do.
///
/// `O_CREAT|O_EXCL` refuses both: it is atomic, and it will not follow a symlink.
fn create_new_in(
    dir: &Path,
    stem: &str,
    ext: &str,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> Result<PathBuf> {
    // Linear probing. A ledger of names already used would not help: the files are on disk
    // by the time the next name is chosen, so it would hold exactly what the directory does.
    // It is quadratic only in photos that share one name, which is a handful in practice.
    let mut n = 1;
    loop {
        let base = if n == 1 {
            stem.to_string()
        } else {
            format!("{stem} ({n})")
        };
        let candidate = dir.join(if ext.is_empty() {
            base
        } else {
            format!("{base}.{ext}")
        });
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                write(&mut file)?;
                return Ok(candidate);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(err) => return Err(err.into()),
        }
    }
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "photo".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{CROP_UNIT, Crop};
    use image::{DynamicImage, ImageFormat, RgbImage};
    use std::io::Cursor;
    use tempfile::TempDir;

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        }));
        let mut bytes = Vec::new();
        img.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Jpeg)
            .unwrap();
        bytes
    }

    fn source(dir: &Path, name: &str, edit: Edit) -> Source {
        let path = dir.join(name);
        std::fs::write(&path, jpeg(8, 4)).unwrap();
        Source {
            path,
            orientation: 1,
            edit,
            width: 8,
            height: 4,
            kind: MediaKind::Image,
        }
    }

    fn quarter_turn() -> Edit {
        Edit::new(1, None).unwrap()
    }

    /// Exactly what `Engine::export_one` does, minus the lock it holds around the render.
    fn export_one(src: &Source, dir: &Path, apply_edits: bool) -> Result<PathBuf> {
        export_with(
            src,
            dir,
            Options {
                apply_edits,
                max_edge: None,
            },
        )
    }

    fn export_with(src: &Source, dir: &Path, options: Options) -> Result<PathBuf> {
        match src.plan(options) {
            Plan::Render { edit, max_edge } => {
                let (bytes, mime) = render_for_export(src, edit, max_edge)?;
                write_rendered(src, dir, mime, &bytes)
            }
            Plan::Copy => copy_original(src, dir),
        }
    }

    #[test]
    fn an_unedited_photo_is_copied_byte_for_byte_in_both_modes() {
        let src_dir = TempDir::new().unwrap();
        let out_dir = TempDir::new().unwrap();
        let src = source(src_dir.path(), "a.jpg", Edit::default());
        let original = std::fs::read(&src.path).unwrap();

        for apply in [false, true] {
            let out = export_one(&src, out_dir.path(), apply).unwrap();
            assert_eq!(
                std::fs::read(&out).unwrap(),
                original,
                "apply_edits={apply}"
            );
        }
    }

    /// The whole point of the checkbox: an edited photo comes out as it is shown, or as the
    /// file it was made from.
    #[test]
    fn an_edited_photo_is_rendered_when_asked_and_copied_when_not() {
        let src_dir = TempDir::new().unwrap();
        let out_dir = TempDir::new().unwrap();
        let src = source(src_dir.path(), "a.jpg", quarter_turn());
        let original = std::fs::read(&src.path).unwrap();

        let rendered = export_one(&src, out_dir.path(), true).unwrap();
        let picture = image::open(&rendered).unwrap();
        assert_eq!(
            (picture.width(), picture.height()),
            (4, 8),
            "a quarter turn of an 8x4 photo"
        );

        let copied = export_one(&src, out_dir.path(), false).unwrap();
        assert_eq!(std::fs::read(&copied).unwrap(), original);
    }

    /// An export is the copy the user keeps, so its colour is kept at full resolution; only
    /// the viewer's throwaway render halves it.
    #[test]
    fn an_exported_render_keeps_its_colour_at_full_resolution() {
        let src_dir = TempDir::new().unwrap();
        let src = source(src_dir.path(), "a.jpg", quarter_turn());
        let (bytes, mime) = render_for_export(&src, src.edit, None).unwrap();
        assert_eq!(mime, "image/jpeg");
        assert_eq!(crate::testutil::jpeg_luma_sampling(&bytes), Some(0x11));
    }

    #[test]
    fn a_name_already_on_disk_is_not_overwritten() {
        let src_dir = TempDir::new().unwrap();
        let out_dir = TempDir::new().unwrap();
        std::fs::write(out_dir.path().join("a.jpg"), b"not a photo").unwrap();
        let src = source(src_dir.path(), "a.jpg", Edit::default());

        let out = export_one(&src, out_dir.path(), false).unwrap();

        assert_eq!(out, out_dir.path().join("a (2).jpg"));
        assert_eq!(
            std::fs::read(out_dir.path().join("a.jpg")).unwrap(),
            b"not a photo"
        );
    }

    /// Two folders, one file name, one export: both copies have to arrive. This holds
    /// `export_one`'s write-then-name order, which is what makes the disk check enough.
    #[test]
    fn two_sources_with_one_name_both_arrive() {
        let a_dir = TempDir::new().unwrap();
        let b_dir = TempDir::new().unwrap();
        let out_dir = TempDir::new().unwrap();
        let a = source(a_dir.path(), "IMG_1234.JPG", Edit::default());
        let b = source(b_dir.path(), "IMG_1234.JPG", Edit::default());

        let first = export_one(&a, out_dir.path(), false).unwrap();
        let second = export_one(&b, out_dir.path(), false).unwrap();

        assert_eq!(first, out_dir.path().join("IMG_1234.JPG"));
        assert_eq!(second, out_dir.path().join("IMG_1234 (2).JPG"));
        assert!(first.exists() && second.exists());
    }

    /// A source the plan can be asked about without a file behind it.
    fn sized(name: &str, width: u32, height: u32, edit: Edit) -> Source {
        Source {
            path: PathBuf::from(name),
            orientation: 1,
            edit,
            width,
            height,
            kind: MediaKind::Image,
        }
    }

    fn within(max_edge: u32, apply_edits: bool) -> Options {
        Options {
            apply_edits,
            max_edge: Some(max_edge),
        }
    }

    #[test]
    fn a_photo_is_copied_unless_an_edit_or_the_size_asks_for_another_picture() {
        let plain = sized("a.jpg", 4000, 3000, Edit::default());
        let no_edit = Edit::default();
        // Within the size, the edge itself included: every byte is kept.
        assert_eq!(plain.plan(within(4000, true)), Plan::Copy);
        assert_eq!(plain.plan(within(8000, true)), Plan::Copy);
        // Over it: scaled, with no edit to apply.
        assert_eq!(
            plain.plan(within(1920, true)),
            Plan::Render {
                edit: no_edit,
                max_edge: Some(1920)
            }
        );
        // Judged by the long edge whichever way the photo lies.
        let portrait = sized("a.jpg", 3000, 4000, Edit::default());
        assert_ne!(portrait.plan(within(3500, true)), Plan::Copy);
        // A size of nothing is no size.
        assert_eq!(plain.plan(within(0, true)), Plan::Copy);
    }

    #[test]
    fn the_size_is_judged_on_the_picture_the_export_writes() {
        // Cropped to the left quarter: 1000 x 3000 as shown.
        let crop = Crop {
            left: 0,
            top: 0,
            right: (CROP_UNIT / 4) as u16,
            bottom: CROP_UNIT as u16,
        };
        let cropped = sized("a.jpg", 4000, 3000, Edit::new(0, Some(crop)).unwrap());
        // Applied, the crop is what is measured: 3000 on its long edge.
        assert_eq!(
            cropped.plan(within(3500, true)),
            Plan::Render {
                edit: cropped.edit,
                max_edge: None
            }
        );
        assert_eq!(
            cropped.plan(within(1920, true)),
            Plan::Render {
                edit: cropped.edit,
                max_edge: Some(1920)
            }
        );
        // Not applied, the file is: 4000, scaled without the crop.
        assert_eq!(
            cropped.plan(within(3500, false)),
            Plan::Render {
                edit: Edit::default(),
                max_edge: Some(3500)
            }
        );
        // And with nothing to scale, the edit left out is a plain copy.
        assert_eq!(cropped.plan(within(4000, false)), Plan::Copy);
        // The crop is of the picture turned upright: a file the camera marks as lying on
        // its side is 3000 x 4000 as shown, and its left quarter 750 x 4000.
        let on_its_side = Source {
            orientation: 6,
            ..cropped.clone()
        };
        assert_eq!(
            on_its_side.plan(within(3500, true)),
            Plan::Render {
                edit: cropped.edit,
                max_edge: Some(3500)
            }
        );
    }

    #[test]
    fn a_video_a_gif_and_a_photo_of_unknown_size_are_never_scaled() {
        let video = Source {
            kind: MediaKind::Video,
            ..sized("clip.mp4", 3840, 2160, Edit::default())
        };
        assert_eq!(video.plan(within(1280, true)), Plan::Copy);
        for name in ["anim.gif", "ANIM.GIF"] {
            let gif = sized(name, 4000, 3000, Edit::default());
            assert_eq!(gif.plan(within(1280, true)), Plan::Copy, "{name}");
        }
        let unread = sized("a.jpg", 0, 0, Edit::default());
        assert_eq!(unread.plan(within(1280, true)), Plan::Copy);
    }

    fn big_source(dir: &Path, name: &str, edit: Edit) -> Source {
        let path = dir.join(name);
        std::fs::write(&path, jpeg(64, 32)).unwrap();
        Source {
            path,
            orientation: 1,
            edit,
            width: 64,
            height: 32,
            kind: MediaKind::Image,
        }
    }

    #[test]
    fn a_photo_over_the_size_comes_out_scaled_and_one_within_it_untouched() {
        let src_dir = TempDir::new().unwrap();
        let out_dir = TempDir::new().unwrap();
        let big = big_source(src_dir.path(), "big.jpg", Edit::default());
        let out = export_with(&big, out_dir.path(), within(16, true)).unwrap();
        let bytes = std::fs::read(&out).unwrap();
        let img = image::load_from_memory(&bytes).unwrap();
        assert_eq!((img.width(), img.height()), (16, 8));
        // A copy to send: colour at half resolution, unlike a full-size render.
        assert_eq!(crate::testutil::jpeg_luma_sampling(&bytes), Some(0x22));

        let small = source(src_dir.path(), "small.jpg", Edit::default());
        let out = export_with(&small, out_dir.path(), within(16, true)).unwrap();
        assert_eq!(
            std::fs::read(&out).unwrap(),
            std::fs::read(&small.path).unwrap()
        );
    }

    #[test]
    fn a_turned_photo_is_scaled_as_it_is_shown() {
        let src_dir = TempDir::new().unwrap();
        let out_dir = TempDir::new().unwrap();
        let turned = big_source(src_dir.path(), "a.jpg", quarter_turn());
        let dims = |options| {
            let out = export_with(&turned, out_dir.path(), options).unwrap();
            let img = image::open(&out).unwrap();
            (img.width(), img.height())
        };
        assert_eq!(dims(within(16, true)), (8, 16));
        assert_eq!(dims(within(16, false)), (16, 8));
    }

    #[test]
    fn a_source_that_cannot_be_read_is_an_error_not_a_panic() {
        let out_dir = TempDir::new().unwrap();
        let src = Source {
            path: PathBuf::from("/nowhere/at/all.jpg"),
            orientation: 1,
            edit: Edit::default(),
            width: 8,
            height: 4,
            kind: MediaKind::Image,
        };
        assert!(export_one(&src, out_dir.path(), false).is_err());
    }

    /// `exists()` follows a symlink, so a dangling one in the destination used to answer
    /// "free" and the write then followed it - straight into the watched folder it pointed
    /// at. `create_new` refuses to follow a link at all, so the name is simply taken and the
    /// copy steps aside to `(2)`.
    #[cfg(unix)]
    #[test]
    fn a_dangling_symlink_in_the_destination_is_never_followed() {
        let src_dir = TempDir::new().unwrap();
        let out_dir = TempDir::new().unwrap();
        let watched = TempDir::new().unwrap();
        let src = source(src_dir.path(), "a.jpg", Edit::default());
        let target = watched.path().join("a.jpg");
        std::os::unix::fs::symlink(&target, out_dir.path().join("a.jpg")).unwrap();

        let out = export_one(&src, out_dir.path(), false).unwrap();

        assert_eq!(out, out_dir.path().join("a (2).jpg"));
        assert!(!target.exists(), "nothing was written through the link");
    }

    /// A crop makes the picture smaller; the export has to carry the crop, not the frame.
    #[test]
    fn a_cropped_photo_comes_out_cropped() {
        let src_dir = TempDir::new().unwrap();
        let out_dir = TempDir::new().unwrap();
        let half = Crop {
            left: 0,
            top: 0,
            right: (CROP_UNIT / 2) as u16,
            bottom: CROP_UNIT as u16,
        };
        let src = source(src_dir.path(), "a.jpg", Edit::new(0, Some(half)).unwrap());

        let out = export_one(&src, out_dir.path(), true).unwrap();

        let picture = image::open(&out).unwrap();
        assert_eq!((picture.width(), picture.height()), (4, 4));
    }
}
