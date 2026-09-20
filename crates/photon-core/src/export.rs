//! Copying photos out of the library.
//!
//! This is the one place photon writes a photo file, and it writes only *new* files, at a
//! destination the user picked. Nothing here opens a watched photo for writing: a source is
//! read, and either its bytes are copied or a fresh picture is encoded beside them.
//!
//! The caller is responsible for refusing a destination inside a watched folder
//! (`Engine::export_items`); this module would happily write there, and the scanner would
//! then index the copies as new photos.

use crate::edit::{Edit, render_full};
use crate::error::Result;
use std::path::{Path, PathBuf};

/// A photo to copy out: where it is, how its EXIF says it is turned, and the edit the user
/// has put on it.
#[derive(Clone, Debug)]
pub struct Source {
    pub path: PathBuf,
    pub orientation: u8,
    pub edit: Edit,
}

/// High enough that the copy is worth keeping. The viewer's own render uses a lower quality
/// on purpose - it is thrown away after one look - and this is the case its doc comment says
/// it is not for.
pub const EXPORT_QUALITY: u8 = 95;

impl Source {
    /// Whether this photo has to be re-encoded to be exported as it is shown. An untouched
    /// photo never does, so the choice between "as shown" and "the original file" only ever
    /// matters for photos the user has edited.
    pub fn needs_render(&self, apply_edits: bool) -> bool {
        apply_edits && !self.edit.is_identity()
    }
}

/// Writes one photo into `dir`, returning the path written.
///
/// Nothing is ever overwritten. A name already in use gets ` (2)`, ` (3)` before its
/// extension - the convention every desktop file manager uses for the same situation.
///
/// Two watched folders can each hold `IMG_1234.JPG`, and both copies have to arrive. They
/// do without a ledger of names already chosen, because each file is *written* before the
/// next name is picked: the first copy is on disk, so the second sees it and steps aside. A
/// ledger was written first and removed when a probe showed it changed no outcome.
pub fn export_one(src: &Source, dir: &Path, apply_edits: bool) -> Result<PathBuf> {
    if src.needs_render(apply_edits) {
        let (bytes, mime) = render_full(&src.path, src.orientation, src.edit, EXPORT_QUALITY)?;
        // The edited picture is no longer the source's format: a JPEG stays a JPEG, but a
        // source with transparency is re-encoded as PNG and anything else photon can decode
        // comes out as one of the two. The extension has to follow the bytes.
        let ext = if mime == "image/png" { "png" } else { "jpg" };
        let out = unique_path(dir, &stem(&src.path), ext);
        std::fs::write(&out, bytes)?;
        Ok(out)
    } else {
        let ext = src
            .path
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        let out = unique_path(dir, &stem(&src.path), &ext);
        // `copy` and not a read-then-write: it keeps every byte, including the EXIF photon
        // does not write, and on most platforms it is a single syscall.
        std::fs::copy(&src.path, &out)?;
        Ok(out)
    }
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "photo".to_string())
}

/// `dir/stem.ext`, or `dir/stem (2).ext` and onwards when that name is already on disk.
fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let named = |n: usize| {
        let base = if n == 1 {
            stem.to_string()
        } else {
            format!("{stem} ({n})")
        };
        dir.join(if ext.is_empty() {
            base
        } else {
            format!("{base}.{ext}")
        })
    };
    let mut n = 1;
    loop {
        let candidate = named(n);
        // `exists()` answers false for a path that cannot be read, which is the right answer
        // here: the write is what decides, and it will report its own error.
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
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
        }
    }

    fn quarter_turn() -> Edit {
        Edit::new(1, None).unwrap()
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

    #[test]
    fn a_source_that_cannot_be_read_is_an_error_not_a_panic() {
        let out_dir = TempDir::new().unwrap();
        let src = Source {
            path: PathBuf::from("/nowhere/at/all.jpg"),
            orientation: 1,
            edit: Edit::default(),
        };
        assert!(export_one(&src, out_dir.path(), false).is_err());
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
