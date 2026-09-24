//! The only `unsafe` code in photon: a wrapper over the dav1d-style API that `rav1d`'s
//! crates.io release exports, which is the only way into it. Each resource it hands out is
//! owned by a guard whose `Drop` gives it back, so no early return can leak one.

use rav1d::include::dav1d::data::Dav1dData;
use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
use rav1d::include::dav1d::headers::{
    DAV1D_PIXEL_LAYOUT_I400, DAV1D_PIXEL_LAYOUT_I420, DAV1D_PIXEL_LAYOUT_I422,
    DAV1D_PIXEL_LAYOUT_I444,
};
use rav1d::include::dav1d::picture::Dav1dPicture;
use rav1d::src::lib::{
    dav1d_close, dav1d_data_create, dav1d_data_unref, dav1d_default_settings, dav1d_get_picture,
    dav1d_open, dav1d_picture_unref, dav1d_send_data,
};
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr::NonNull;

/// The largest frame rav1d is allowed to allocate, in pixels: `image`'s 512 MiB decode
/// bound at four bytes a pixel, so a header claiming absurd dimensions is refused up front.
const MAX_FRAME_PIXELS: u32 = 512 * 1024 * 1024 / 4;

/// dav1d reports "not now" as `-EAGAIN`, and `EAGAIN` is 11 on Linux and Windows but 35 on
/// macOS: it has to come from the platform's own headers.
const AGAIN: i32 = -libc::EAGAIN;

/// One decoded AV1 frame. Every plane is widened to 16-bit samples whatever the bit depth,
/// and packed row after row with no stride padding.
pub(crate) struct Planes {
    pub width: usize,
    pub height: usize,
    /// 8, 10 or 12.
    pub depth: u8,
    /// No chroma planes: `u` and `v` are empty.
    pub mono: bool,
    /// Chroma subsampling as (x, y) shifts: (1, 1) is 4:2:0, (1, 0) 4:2:2, (0, 0) 4:4:4.
    pub shift: (u32, u32),
    pub y: Vec<u16>,
    pub u: Vec<u16>,
    pub v: Vec<u16>,
    /// H.273 matrix coefficients and range, as the AV1 sequence header states them.
    pub matrix: u16,
    pub full_range: bool,
}

impl Planes {
    pub fn chroma_width(&self) -> usize {
        (self.width + (1 << self.shift.0) - 1) >> self.shift.0
    }

    pub fn chroma_height(&self) -> usize {
        (self.height + (1 << self.shift.1) - 1) >> self.shift.1
    }
}

struct Context(Option<Dav1dContext>);

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: `self.0` is either `None`, which `dav1d_close` ignores, or the context
        // `dav1d_open` wrote there, which nothing else holds.
        unsafe { dav1d_close(NonNull::new(&mut self.0)) }
    }
}

struct Data(Dav1dData);

impl Drop for Data {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from `dav1d_data_create`, or is what `dav1d_send_data` left
        // of it; unreferencing an emptied buffer is a no-op.
        unsafe { dav1d_data_unref(NonNull::new(&mut self.0)) }
    }
}

struct Picture(Dav1dPicture);

impl Drop for Picture {
    fn drop(&mut self) {
        // SAFETY: a `Picture` is only built from a picture `dav1d_get_picture` returned.
        unsafe { dav1d_picture_unref(NonNull::new(&mut self.0)) }
    }
}

/// Decodes one AV1 item - a still image, a grid tile or an alpha plane - to its planes.
pub(crate) fn decode(obu: &[u8]) -> Result<Planes, String> {
    if obu.is_empty() {
        return Err("empty AV1 item".into());
    }
    let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
    // SAFETY: `dav1d_default_settings` writes every field of the struct it is pointed at.
    let mut settings = unsafe {
        dav1d_default_settings(NonNull::new_unchecked(settings.as_mut_ptr()));
        settings.assume_init()
    };
    // One thread: the thumbnail pool already decodes `MAX_WORKERS` photos at once, and
    // rav1d's default of a thread per core under each of them would oversubscribe the machine.
    settings.n_threads = 1;
    settings.max_frame_delay = 1;
    settings.frame_size_limit = MAX_FRAME_PIXELS;

    let mut context = Context(None);
    // SAFETY: both pointers are to live locals; on success the new context is owned by
    // `context`, whose `Drop` closes it.
    let opened = unsafe { dav1d_open(NonNull::new(&mut context.0), NonNull::new(&mut settings)) };
    if opened.0 != 0 {
        return Err(format!("could not start the AV1 decoder ({})", opened.0));
    }

    // SAFETY: all-zero is a valid, empty `Dav1dData`: its pointers are `Option`s and the
    // rest are plain integers.
    let mut data = Data(unsafe { std::mem::zeroed() });
    // SAFETY: `data_create` initialises `data` and returns a buffer of `obu.len()` bytes
    // that `data` owns, or null when it cannot allocate.
    let buffer = unsafe { dav1d_data_create(NonNull::new(&mut data.0), obu.len()) };
    if buffer.is_null() {
        return Err("out of memory for the AV1 item".into());
    }
    // SAFETY: `buffer` is a fresh allocation of exactly `obu.len()` bytes that nothing else
    // can reach yet, and `obu` cannot overlap it.
    unsafe { std::ptr::copy_nonoverlapping(obu.as_ptr(), buffer, obu.len()) };

    let picture = loop {
        if data.0.sz > 0 {
            // SAFETY: `context.0` is the open decoder and `data` a live buffer from
            // `data_create`; `send_data` takes what it consumes and shrinks `sz`.
            let sent = unsafe { dav1d_send_data(context.0, NonNull::new(&mut data.0)) };
            if sent.0 != 0 && sent.0 != AGAIN {
                return Err(format!("the AV1 decoder refused the item ({})", sent.0));
            }
        }
        // SAFETY: all-zero is a valid empty `Dav1dPicture`, which `get_picture` overwrites.
        let mut out: Dav1dPicture = unsafe { std::mem::zeroed() };
        // SAFETY: `context.0` is the open decoder and `out` a live local.
        let got = unsafe { dav1d_get_picture(context.0, NonNull::new(&mut out)) };
        if got.0 == 0 {
            break Picture(out);
        }
        // "Not now" with input still to send means send it; with nothing left, the item
        // held no frame at all. Without the second half this loop would spin forever on a
        // truncated item.
        if got.0 != AGAIN || data.0.sz == 0 {
            return Err(format!("the AV1 item decoded to no picture ({})", got.0));
        }
    };
    planes(&picture.0)
}

fn planes(picture: &Dav1dPicture) -> Result<Planes, String> {
    let p = &picture.p;
    let (width, height, depth) = (p.w as usize, p.h as usize, p.bpc as u8);
    let (mono, shift) = match p.layout {
        DAV1D_PIXEL_LAYOUT_I400 => (true, (0, 0)),
        DAV1D_PIXEL_LAYOUT_I420 => (false, (1, 1)),
        DAV1D_PIXEL_LAYOUT_I422 => (false, (1, 0)),
        DAV1D_PIXEL_LAYOUT_I444 => (false, (0, 0)),
        other => return Err(format!("unknown AV1 pixel layout {other}")),
    };
    if !matches!(depth, 8 | 10 | 12) || width == 0 || height == 0 {
        return Err(format!(
            "unusable AV1 picture: {width}x{height} at {depth} bits"
        ));
    }
    let header = picture
        .seq_hdr
        .ok_or("AV1 picture without a sequence header")?;
    // SAFETY: a returned picture holds a reference to its sequence header, alive until the
    // picture is unreferenced, which cannot happen while `picture` is borrowed here.
    let header = unsafe { header.as_ref() };
    let mut planes = Planes {
        width,
        height,
        depth,
        mono,
        shift,
        y: Vec::new(),
        u: Vec::new(),
        v: Vec::new(),
        matrix: header.mtrx as u16,
        full_range: header.color_range != 0,
    };
    planes.y = copy_plane(picture.data[0], picture.stride[0], width, height, depth)?;
    if !mono {
        let (cw, ch) = (planes.chroma_width(), planes.chroma_height());
        planes.u = copy_plane(picture.data[1], picture.stride[1], cw, ch, depth)?;
        planes.v = copy_plane(picture.data[2], picture.stride[1], cw, ch, depth)?;
    }
    Ok(planes)
}

fn copy_plane(
    data: Option<NonNull<c_void>>,
    stride: isize,
    width: usize,
    height: usize,
    depth: u8,
) -> Result<Vec<u16>, String> {
    let base = data.ok_or("AV1 picture missing a plane")?.as_ptr() as *const u8;
    let mut out = Vec::with_capacity(width * height);
    for row in 0..height {
        // SAFETY: dav1d lays a plane out as `height` rows `stride` bytes apart, each holding
        // `width` samples of one byte at 8 bits and two (aligned) above, and the picture
        // outlives this copy.
        unsafe {
            let start = base.offset(row as isize * stride);
            if depth == 8 {
                let samples = std::slice::from_raw_parts(start, width);
                out.extend(samples.iter().map(|&s| u16::from(s)));
            } else {
                let samples = std::slice::from_raw_parts(start as *const u16, width);
                out.extend_from_slice(samples);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::avif_fixture;

    /// The AV1 payload of a single-image fixture.
    fn primary(name: &str) -> Vec<u8> {
        let bytes = avif_fixture(name);
        let parser = zenavif_parse::AvifParser::from_bytes(&bytes).unwrap();
        parser.primary_data().unwrap().into_owned()
    }

    #[test]
    fn decodes_an_item_to_its_planes() {
        let p = decode(&primary("red_blue_709_limited.avif")).unwrap();
        assert_eq!((p.width, p.height, p.depth), (64, 32, 8));
        assert_eq!((p.mono, p.shift), (false, (1, 1)));
        assert_eq!((p.chroma_width(), p.chroma_height()), (32, 16));
        assert_eq!(
            (p.y.len(), p.u.len(), p.v.len()),
            (64 * 32, 32 * 16, 32 * 16)
        );
        // The sequence header's own colour description: BT.709, limited range.
        assert_eq!((p.matrix, p.full_range), (1, false));
    }

    #[test]
    fn reports_each_chroma_layout() {
        let layout = |name| {
            let p = decode(&primary(name)).unwrap();
            (p.mono, p.shift, p.u.len())
        };
        assert_eq!(layout("red_blue_444_full.avif"), (false, (0, 0), 64 * 32));
        assert_eq!(layout("top_bottom_422.avif"), (false, (1, 0), 16 * 32));
        assert_eq!(layout("mono.avif"), (true, (0, 0), 0));
    }

    /// 33×17 at 4:2:0 has a 17×9 chroma plane: rounded up, not down.
    #[test]
    fn an_odd_size_rounds_the_chroma_plane_up() {
        let p = decode(&primary("odd_420.avif")).unwrap();
        assert_eq!((p.width, p.height), (33, 17));
        assert_eq!((p.chroma_width(), p.chroma_height()), (17, 9));
        assert_eq!(p.u.len(), 17 * 9);
    }

    /// Ten-bit samples arrive whole. Read as bytes, white would top out at 255 rather than
    /// near 1023.
    #[test]
    fn keeps_samples_deeper_than_eight_bits() {
        let bytes = avif_fixture("grid_10bit.avif");
        let parser = zenavif_parse::AvifParser::from_bytes(&bytes).unwrap();
        // Tile 3 is the white quadrant.
        let p = decode(&parser.tile_data(3).unwrap()).unwrap();
        assert_eq!(p.depth, 10);
        assert!(
            p.y.iter().copied().max().unwrap() > 900,
            "white luma should be near 1023"
        );
    }

    #[test]
    fn refuses_garbage_and_empty_items() {
        assert!(decode(&[]).is_err());
        assert!(decode(b"definitely not AV1 at all").is_err());
        let mut cut = primary("red_blue_709_limited.avif");
        cut.truncate(cut.len() / 2);
        assert!(decode(&cut).is_err());
    }
}
