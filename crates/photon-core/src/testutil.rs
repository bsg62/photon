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
        rating: None,
        camera: crate::metadata::CameraMeta::default(),
        tags: Vec::new(),
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

/// The EXIF fields the fixture builder can write. Every field is optional so a test names
/// only what it is about.
#[derive(Clone, Debug, Default)]
pub struct ExifSpec<'a> {
    pub orientation: Option<u16>,
    /// `DateTimeOriginal`, exactly "YYYY:MM:DD HH:MM:SS".
    pub datetime: Option<&'a str>,
    /// `DateTime` (the file-change date in IFD0), same format.
    pub modified: Option<&'a str>,
    /// `ImageDescription` (IFD0 0x010E), ASCII - what cameras fill with their own name.
    pub description: Option<&'a str>,
    pub make: Option<&'a str>,
    pub model: Option<&'a str>,
    pub lens: Option<&'a str>,
    /// `FocalLength` as a rational (numerator, denominator).
    pub focal: Option<(u32, u32)>,
    /// `FNumber` as a rational.
    pub fnumber: Option<(u32, u32)>,
    /// `ExposureTime` as a rational.
    pub exposure: Option<(u32, u32)>,
    /// `PhotographicSensitivity`, a SHORT.
    pub iso: Option<u16>,
}

/// One IFD entry: tag, TIFF type, count and the raw value bytes (little-endian).
struct IfdEntry {
    tag: u16,
    typ: u16,
    count: u32,
    data: Vec<u8>,
}

fn ascii_entry(tag: u16, text: &str) -> IfdEntry {
    let mut data = text.as_bytes().to_vec();
    data.push(0);
    IfdEntry {
        tag,
        typ: 2,
        count: data.len() as u32,
        data,
    }
}

fn short_entry(tag: u16, value: u16) -> IfdEntry {
    IfdEntry {
        tag,
        typ: 3,
        count: 1,
        data: value.to_le_bytes().to_vec(),
    }
}

fn long_entry(tag: u16, value: u32) -> IfdEntry {
    IfdEntry {
        tag,
        typ: 4,
        count: 1,
        data: value.to_le_bytes().to_vec(),
    }
}

fn rational_entry(tag: u16, (num, denom): (u32, u32)) -> IfdEntry {
    let mut data = num.to_le_bytes().to_vec();
    data.extend_from_slice(&denom.to_le_bytes());
    IfdEntry {
        tag,
        typ: 5,
        count: 1,
        data,
    }
}

/// Serialises one IFD starting at `base` (an offset into the TIFF): the entry table, then
/// the data area for values longer than four bytes. Entries are sorted by tag as the
/// specification asks. Returns the bytes and the offset just past them.
fn write_ifd(mut entries: Vec<IfdEntry>, base: u32) -> (Vec<u8>, u32) {
    entries.sort_by_key(|e| e.tag);
    let table_len = 2 + entries.len() * 12 + 4;
    let mut table = Vec::with_capacity(table_len);
    let mut data: Vec<u8> = Vec::new();
    table.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for e in &entries {
        table.extend_from_slice(&e.tag.to_le_bytes());
        table.extend_from_slice(&e.typ.to_le_bytes());
        table.extend_from_slice(&e.count.to_le_bytes());
        if e.data.len() <= 4 {
            let mut value = [0u8; 4];
            value[..e.data.len()].copy_from_slice(&e.data);
            table.extend_from_slice(&value);
        } else {
            let offset = base + table_len as u32 + data.len() as u32;
            table.extend_from_slice(&offset.to_le_bytes());
            data.extend_from_slice(&e.data);
            if data.len() % 2 == 1 {
                data.push(0);
            }
        }
    }
    table.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    table.extend_from_slice(&data);
    let end = base + table.len() as u32;
    (table, end)
}

/// A little-endian TIFF structure with IFD0 and an Exif sub-IFD holding `spec`'s fields.
pub fn exif_tiff(spec: &ExifSpec<'_>) -> Vec<u8> {
    let mut exif_ifd = Vec::new();
    if let Some(dt) = spec.datetime {
        assert_eq!(dt.len(), 19, "datetime must be YYYY:MM:DD HH:MM:SS");
        exif_ifd.push(ascii_entry(0x9003, dt));
    }
    if let Some(r) = spec.exposure {
        exif_ifd.push(rational_entry(0x829a, r));
    }
    if let Some(r) = spec.fnumber {
        exif_ifd.push(rational_entry(0x829d, r));
    }
    if let Some(iso) = spec.iso {
        exif_ifd.push(short_entry(0x8827, iso));
    }
    if let Some(r) = spec.focal {
        exif_ifd.push(rational_entry(0x920a, r));
    }
    if let Some(lens) = spec.lens {
        exif_ifd.push(ascii_entry(0xa434, lens));
    }

    let mut ifd0 = Vec::new();
    if let Some(description) = spec.description {
        ifd0.push(ascii_entry(0x010e, description));
    }
    if let Some(make) = spec.make {
        ifd0.push(ascii_entry(0x010f, make));
    }
    if let Some(model) = spec.model {
        ifd0.push(ascii_entry(0x0110, model));
    }
    if let Some(o) = spec.orientation {
        ifd0.push(short_entry(0x0112, o));
    }
    if let Some(dt) = spec.modified {
        assert_eq!(dt.len(), 19, "modified must be YYYY:MM:DD HH:MM:SS");
        ifd0.push(ascii_entry(0x0132, dt));
    }
    // The pointer's value is the offset of the Exif IFD, which sits right after IFD0 and
    // its data; lay IFD0 out once with a placeholder to learn its length.
    let has_exif_ifd = !exif_ifd.is_empty();
    if has_exif_ifd {
        ifd0.push(long_entry(0x8769, 0));
    }
    let (_, exif_offset) = write_ifd(
        ifd0.iter()
            .map(|e| IfdEntry {
                tag: e.tag,
                typ: e.typ,
                count: e.count,
                data: e.data.clone(),
            })
            .collect(),
        8,
    );
    if has_exif_ifd {
        let pointer = ifd0.iter_mut().find(|e| e.tag == 0x8769).unwrap();
        pointer.data = exif_offset.to_le_bytes().to_vec();
    }

    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II\x2a\x00");
    tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD0 offset
    let (ifd0_bytes, _) = write_ifd(ifd0, 8);
    tiff.extend_from_slice(&ifd0_bytes);
    if has_exif_ifd {
        assert_eq!(tiff.len() as u32, exif_offset);
        let (exif_bytes, _) = write_ifd(exif_ifd, exif_offset);
        tiff.extend_from_slice(&exif_bytes);
    }
    tiff
}

/// A JPEG with the given APP segments (marker byte, payload) inserted right after SOI.
pub fn jpeg_with_segments(w: u32, h: u32, segments: &[(u8, &[u8])]) -> Vec<u8> {
    let jpeg = jpeg_bytes(w, h);
    let mut out = jpeg[..2].to_vec(); // SOI
    for (marker, payload) in segments {
        out.extend_from_slice(&[0xFF, *marker]);
        out.extend_from_slice(&((2 + payload.len()) as u16).to_be_bytes());
        out.extend_from_slice(payload);
    }
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// A JPEG carrying `spec` in an APP1 EXIF segment.
pub fn jpeg_with_exif_spec(w: u32, h: u32, spec: &ExifSpec<'_>) -> Vec<u8> {
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(&exif_tiff(spec));
    jpeg_with_segments(w, h, &[(0xE1, &app1)])
}

/// A JPEG carrying a minimal EXIF block with Orientation and DateTimeOriginal.
/// `datetime` must be exactly "YYYY:MM:DD HH:MM:SS".
pub fn jpeg_with_exif(w: u32, h: u32, orientation: u16, datetime: &str) -> Vec<u8> {
    jpeg_with_exif_spec(
        w,
        h,
        &ExifSpec {
            orientation: Some(orientation),
            datetime: Some(datetime),
            ..ExifSpec::default()
        },
    )
}

/// An APP13 "Photoshop 3.0" segment payload holding one IPTC-NAA resource with the given
/// keywords as dataset 2:25 records, each in the given raw bytes.
pub fn iptc_app13(keywords: &[&[u8]]) -> Vec<u8> {
    iptc_app13_datasets(&keywords.iter().map(|k| (25, *k)).collect::<Vec<_>>())
}

/// An APP13 payload carrying the given IIM record-2 datasets in order, e.g.
/// `&[(120, b"caption"), (25, b"keyword")]`.
pub fn iptc_app13_datasets(datasets: &[(u8, &[u8])]) -> Vec<u8> {
    let mut iim = Vec::new();
    for (dataset, value) in datasets {
        iim.extend_from_slice(&[0x1C, 2, *dataset]);
        iim.extend_from_slice(&(value.len() as u16).to_be_bytes());
        iim.extend_from_slice(value);
    }
    let mut payload = b"Photoshop 3.0\0".to_vec();
    payload.extend_from_slice(b"8BIM");
    payload.extend_from_slice(&0x0404u16.to_be_bytes());
    payload.extend_from_slice(&[0, 0]); // empty Pascal name, padded to two bytes
    payload.extend_from_slice(&(iim.len() as u32).to_be_bytes());
    payload.extend_from_slice(&iim);
    if iim.len() % 2 == 1 {
        payload.push(0);
    }
    payload
}

/// A JPEG whose IPTC block carries `keywords`.
pub fn jpeg_with_iptc_keywords(w: u32, h: u32, keywords: &[&[u8]]) -> Vec<u8> {
    jpeg_with_segments(w, h, &[(0xED, &iptc_app13(keywords))])
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
    xmp_packet_with(&format!(r#"xmp:Rating="{rating}""#), "")
}

/// An XMP packet whose `dc:subject` bag lists `subjects`, each XML-escaped.
pub fn xmp_packet_with_subjects(subjects: &[&str]) -> String {
    let items: String = subjects
        .iter()
        .map(|s| {
            let escaped = s
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            format!("<rdf:li>{escaped}</rdf:li>")
        })
        .collect();
    xmp_packet_with(
        r#"xmlns:dc="http://purl.org/dc/elements/1.1/""#,
        &format!("<dc:subject><rdf:Bag>{items}</rdf:Bag></dc:subject>"),
    )
}

/// An XMP packet whose `dc:description` `rdf:Alt` holds `(xml:lang, text)` entries in order,
/// the text XML-escaped. A `None` language writes the `rdf:li` without the attribute.
pub fn xmp_packet_with_description(entries: &[(Option<&str>, &str)]) -> String {
    let items: String = entries
        .iter()
        .map(|(lang, text)| {
            let escaped = text
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            match lang {
                Some(lang) => format!(r#"<rdf:li xml:lang="{lang}">{escaped}</rdf:li>"#),
                None => format!("<rdf:li>{escaped}</rdf:li>"),
            }
        })
        .collect();
    xmp_packet_with(
        r#"xmlns:dc="http://purl.org/dc/elements/1.1/""#,
        &format!("<dc:description><rdf:Alt>{items}</rdf:Alt></dc:description>"),
    )
}

fn xmp_packet_with(attributes: &str, children: &str) -> String {
    format!(
        r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/" {attributes}>{children}</rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#
    )
}

/// A JPEG carrying `packet` in an APP1 XMP segment.
pub fn jpeg_with_xmp_packet(w: u32, h: u32, packet: &str) -> Vec<u8> {
    let mut app1 = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    app1.extend_from_slice(packet.as_bytes());
    jpeg_with_segments(w, h, &[(0xE1, &app1)])
}

/// A JPEG carrying the packet in an APP1 segment, as a camera or Picasa writes it.
pub fn jpeg_with_xmp(w: u32, h: u32, rating: i32) -> Vec<u8> {
    jpeg_with_xmp_packet(w, h, &xmp_packet(rating))
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
/// still sees contiguous XML, which is exactly what `xmp::read_rating` relies on. To keep
/// the file a byte-valid, decodable GIF, the raw packet is followed by the standard 256-byte
/// "magic trailer": for i in 0..255, a byte valued `254 - i`, plus one final `0x00`. A GIF
/// decoder that parses the extension strictly as length-prefixed sub-blocks (rather than
/// reading the XMP packet as one contiguous blob) walks arbitrary "lengths" while stepping
/// through the XML content, landing at some unpredictable offset inside the trailer; because
/// each position i there is worth exactly `254 - i`, that jump always lands on the very last
/// trailer byte (value 0), which is read as a proper zero-length terminator, however
/// desynchronized the walk through the XML was.
pub fn gif_with_xmp(w: u32, h: u32, rating: i32) -> Vec<u8> {
    let packet = xmp_packet(rating);
    let mut ext = vec![0x21, 0xFF, 0x0B];
    ext.extend_from_slice(b"XMP DataXMP");
    ext.extend_from_slice(packet.as_bytes());
    ext.extend((0..=254u8).rev());
    ext.push(0x00);

    let gif = encode(&solid(w, h), ImageFormat::Gif);
    // 6-byte header + 7-byte logical screen descriptor. If the descriptor's packed field
    // has its top bit set, a global colour table of 3 * 2^(N+1) bytes follows, N being the
    // low three bits. The encoder does emit one, so a fixed offset splices into the middle
    // of the table and yields a byte-invalid GIF.
    let packed = gif[10];
    let gct = if packed & 0x80 != 0 {
        3 * (1usize << ((packed & 0x07) + 1))
    } else {
        0
    };
    let split = 13 + gct;
    let mut out = gif[..split].to_vec();
    out.extend_from_slice(&ext);
    out.extend_from_slice(&gif[split..]);
    out
}

/// One IFD entry holding several SHORTs, for a field like `BitsPerSample`.
fn shorts_entry(tag: u16, values: &[u16]) -> IfdEntry {
    let mut data = Vec::with_capacity(values.len() * 2);
    for v in values {
        data.extend_from_slice(&v.to_le_bytes());
    }
    IfdEntry {
        tag,
        typ: 3,
        count: values.len() as u32,
        data,
    }
}

/// A baseline TIFF: little-endian, uncompressed RGB, one strip, no EXIF.
///
/// Hand-built rather than encoded by `image`, so the fixture exists whether or not the
/// crate's `tiff` feature is on. That is what lets a test of "photon reads TIFFs" fail on
/// its assertion rather than on a missing encoder.
pub fn tiff_bytes(w: u32, h: u32) -> Vec<u8> {
    let pixels: Vec<u8> = (0..w * h).flat_map(|_| [200u8, 100, 50]).collect();
    const PIXEL_OFFSET: u32 = 8;
    let ifd_base = PIXEL_OFFSET + pixels.len() as u32;
    let entries = vec![
        long_entry(0x0100, w),                   // ImageWidth
        long_entry(0x0101, h),                   // ImageLength
        shorts_entry(0x0102, &[8, 8, 8]),        // BitsPerSample
        short_entry(0x0103, 1),                  // Compression: none
        short_entry(0x0106, 2),                  // PhotometricInterpretation: RGB
        long_entry(0x0111, PIXEL_OFFSET),        // StripOffsets
        short_entry(0x0115, 3),                  // SamplesPerPixel
        long_entry(0x0116, h),                   // RowsPerStrip: the whole image
        long_entry(0x0117, pixels.len() as u32), // StripByteCounts
        short_entry(0x011C, 1),                  // PlanarConfiguration: chunky
    ];
    let (ifd, _) = write_ifd(entries, ifd_base);

    let mut out = Vec::with_capacity(ifd_base as usize + ifd.len());
    out.extend_from_slice(b"II\x2a\x00");
    out.extend_from_slice(&ifd_base.to_le_bytes());
    out.extend_from_slice(&pixels);
    out.extend_from_slice(&ifd);
    out
}

/// A 24-bit BMP with a `BITMAPINFOHEADER`, stored bottom-up as the format's default is.
/// Hand-built for the same reason as `tiff_bytes`.
pub fn bmp_bytes(w: u32, h: u32) -> Vec<u8> {
    const HEADER_LEN: u32 = 54;
    // Every row is padded out to a four-byte boundary.
    let stride = (w * 3).div_ceil(4) * 4;
    let mut pixels = vec![0u8; (stride * h) as usize];
    for row in pixels.chunks_exact_mut(stride as usize) {
        for px in row[..(w * 3) as usize].as_chunks_mut::<3>().0 {
            *px = [50, 100, 200]; // BGR of the same colour `solid` uses
        }
    }

    let mut out = Vec::with_capacity(HEADER_LEN as usize + pixels.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(HEADER_LEN + pixels.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // reserved
    out.extend_from_slice(&HEADER_LEN.to_le_bytes()); // offset to the pixels
    out.extend_from_slice(&40u32.to_le_bytes()); // BITMAPINFOHEADER size
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(h as i32).to_le_bytes()); // positive: bottom-up
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&24u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB, uncompressed
    out.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes()); // 72 dpi horizontally
    out.extend_from_slice(&2835i32.to_le_bytes()); // and vertically
    out.extend_from_slice(&0u32.to_le_bytes()); // palette colours used
    out.extend_from_slice(&0u32.to_le_bytes()); // and important
    out.extend_from_slice(&pixels);
    out
}

/// One of the `avifenc`-made files in `testdata/avif` (see its README for what each holds).
pub fn avif_fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/avif")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}
