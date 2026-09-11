#![allow(dead_code)]

use crate::library::{Library, NewItem};
use crate::media::MediaKind;
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A fresh library in its own temporary directory. Keep the `TempDir` alive for the test.
pub fn temp_library() -> (TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(&dir.path().join("library.db")).unwrap();
    (dir, lib)
}

/// Registers `path` as a watched folder with a root folder row. Returns (watched_id, folder_id).
pub fn seed_folder(lib: &Library, path: &Path) -> (i64, i64) {
    let watched = watch(lib, path.to_str().unwrap());
    let folder = lib
        .upsert_folder(watched.id, None, path.to_str().unwrap(), 1)
        .unwrap();
    (watched.id, folder)
}

/// Inserts a watched-folder row for a synthetic path, without the filesystem validation
/// `Library::add_watched_folder` performs.
pub fn watch(lib: &Library, path: &str) -> crate::library::WatchedFolder {
    lib.register_watched_folder(path).unwrap()
}

pub fn new_item(folder_id: i64, path: &str, taken_at: i64) -> NewItem {
    NewItem {
        folder_id,
        path: path.to_string(),
        file_name: Path::new(path)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string(),
        kind: MediaKind::Image,
        size: 100,
        mtime_ms: 1_000,
        width: 400,
        height: 300,
        orientation: 1,
        taken_at,
    }
}

pub fn encode(img: &DynamicImage, format: ImageFormat) -> Vec<u8> {
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), format).unwrap();
    buf
}

fn solid(w: u32, h: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, Rgb([200, 100, 50])))
}

pub fn jpeg_bytes(w: u32, h: u32) -> Vec<u8> {
    encode(&solid(w, h), ImageFormat::Jpeg)
}

pub fn png_bytes(w: u32, h: u32) -> Vec<u8> {
    encode(&solid(w, h), ImageFormat::Png)
}

/// A JPEG carrying a minimal little-endian EXIF block with Orientation and DateTimeOriginal.
/// `datetime` must be exactly "YYYY:MM:DD HH:MM:SS".
pub fn jpeg_with_exif(w: u32, h: u32, orientation: u16, datetime: &str) -> Vec<u8> {
    fn entry(t: &mut Vec<u8>, tag: u16, typ: u16, count: u32, value: u32) {
        t.extend_from_slice(&tag.to_le_bytes());
        t.extend_from_slice(&typ.to_le_bytes());
        t.extend_from_slice(&count.to_le_bytes());
        t.extend_from_slice(&value.to_le_bytes());
    }
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II\x2a\x00");
    tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD0 offset
    tiff.extend_from_slice(&2u16.to_le_bytes()); // IFD0: 2 entries
    entry(&mut tiff, 0x0112, 3, 1, orientation as u32); // Orientation, SHORT
    entry(&mut tiff, 0x8769, 4, 1, 38); // Exif IFD pointer, LONG
    tiff.extend_from_slice(&0u32.to_le_bytes()); // no IFD1
    tiff.extend_from_slice(&1u16.to_le_bytes()); // Exif IFD at 38: 1 entry
    entry(&mut tiff, 0x9003, 2, 20, 56); // DateTimeOriginal, ASCII[20] at 56
    tiff.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(tiff.len(), 56);
    let mut date = datetime.as_bytes().to_vec();
    date.push(0);
    assert_eq!(date.len(), 20, "datetime must be YYYY:MM:DD HH:MM:SS");
    tiff.extend_from_slice(&date);

    let mut app1 = vec![0xFF, 0xE1];
    app1.extend_from_slice(&((2 + 6 + tiff.len()) as u16).to_be_bytes());
    app1.extend_from_slice(b"Exif\0\0");
    app1.extend_from_slice(&tiff);

    let jpeg = jpeg_bytes(w, h);
    let mut out = jpeg[..2].to_vec(); // SOI
    out.extend_from_slice(&app1);
    out.extend_from_slice(&jpeg[2..]);
    out
}

pub fn write_file(dir: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let path = rel
        .split('/')
        .fold(dir.to_path_buf(), |p, part| p.join(part));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    path
}
