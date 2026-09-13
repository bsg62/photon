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

/// A minimal XMP packet carrying `xmp:Rating` as an attribute, the spelling Picasa writes.
pub fn xmp_packet(rating: i32) -> String {
    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="{rating}"/>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#
    )
}

/// A JPEG carrying the packet in an APP1 segment, as a camera or Picasa writes it.
pub fn jpeg_with_xmp(w: u32, h: u32, rating: i32) -> Vec<u8> {
    let packet = xmp_packet(rating);
    let mut app1 = vec![0xFF, 0xE1];
    let ns = b"http://ns.adobe.com/xap/1.0/\0";
    app1.extend_from_slice(&((2 + ns.len() + packet.len()) as u16).to_be_bytes());
    app1.extend_from_slice(ns);
    app1.extend_from_slice(packet.as_bytes());

    let jpeg = jpeg_bytes(w, h);
    let mut out = jpeg[..2].to_vec(); // SOI
    out.extend_from_slice(&app1);
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// CRC-32 (IEEE), computed bitwise so no table or dependency is needed. PNG chunks carry one.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A PNG carrying the packet in an uncompressed `iTXt` chunk, inserted after the IHDR.
pub fn png_with_xmp(w: u32, h: u32, rating: i32) -> Vec<u8> {
    let packet = xmp_packet(rating);
    let mut data = Vec::new();
    data.extend_from_slice(b"XML:com.adobe.xmp\0"); // keyword + null
    data.push(0); // compression flag: uncompressed
    data.push(0); // compression method
    data.push(0); // language tag: empty, null-terminated
    data.push(0); // translated keyword: empty, null-terminated
    data.extend_from_slice(packet.as_bytes());

    let mut chunk = Vec::new();
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut typed = b"iTXt".to_vec();
    typed.extend_from_slice(&data);
    chunk.extend_from_slice(&typed);
    chunk.extend_from_slice(&crc32(&typed).to_be_bytes());

    // 8-byte signature, then IHDR (4 len + 4 type + 13 data + 4 crc = 25 bytes).
    let png = png_bytes(w, h);
    let split = 8 + 25;
    let mut out = png[..split].to_vec();
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&png[split..]);
    out
}

/// A GIF carrying the packet in an XMP Application Extension, inserted after the header.
/// The XMP GIF convention stores the packet so that a reader ignoring sub-block framing
/// still sees contiguous XML, which is exactly what `xmp::read_rating` relies on.
pub fn gif_with_xmp(w: u32, h: u32, rating: i32) -> Vec<u8> {
    let packet = xmp_packet(rating);
    let mut ext = vec![0x21, 0xFF, 0x0B];
    ext.extend_from_slice(b"XMP DataXMP");
    ext.extend_from_slice(packet.as_bytes());
    ext.push(0x00); // block terminator

    let gif = encode(&solid(w, h), ImageFormat::Gif);
    // Header (6) + logical screen descriptor (7). No global colour table is emitted for
    // these solid images; if one were present it would follow and the packet would simply
    // sit after it, which the scan also tolerates.
    let split = 13.min(gif.len());
    let mut out = gif[..split].to_vec();
    out.extend_from_slice(&ext);
    out.extend_from_slice(&gif[split..]);
    out
}
