//! The only `unsafe` code in photon-core: a wrapper over the dav1d-style API that `rav1d`'s
//! crates.io release exports, which is the only way into it. Each resource it hands out is
//! owned by a guard whose `Drop` gives it back, so no early return can leak one.
//!
//! A panic inside rav1d cannot be caught the way every other format's decoder panic is
//! (`thumbs/service.rs`'s `catch_unwind`): rav1d's Rust functions are `pub unsafe extern "C"
//! fn`s, and since Rust 1.81 a panic unwinding out of an `extern "C"` boundary aborts the
//! process instead of unwinding through it - a language guarantee, not a bug to route around.
//! The crates.io release exports no other entry point (every other Rust function in the crate
//! is `pub(crate)`), so there is no safe-to-unwind call available to reach for instead. An AVIF
//! that trips a rav1d panic therefore aborts photon, where every other format's decoder panic
//! costs one thumbnail. This is accepted rather than hidden; a crash-loop guard around opening
//! an AVIF-heavy library is a possible follow-up.

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
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ptr::NonNull;

/// The largest frame rav1d is allowed to allocate, in pixels: the same pixel count as
/// `image`'s 512 MiB RGBA8 decode bound, so a header claiming absurd dimensions is refused up
/// front on the same footing as any other format - not a memory bound on AV1 decoding itself,
/// which costs more per pixel at this cap than RGBA8 does (10/12-bit 4:4:4 planes inside
/// rav1d, then `Planes` widening every sample to 2 bytes).
const MAX_FRAME_PIXELS: u32 = 512 * 1024 * 1024 / 4;

/// dav1d reports "not now" as `-EAGAIN`, and `EAGAIN` is 11 on Linux and Windows but 35 on
/// macOS: it has to come from the platform's own headers.
const AGAIN: i32 = -libc::EAGAIN;

/// One row of decoded samples borrowed directly from a `Dav1dPicture` plane (or a test
/// buffer): 8-bit frames stay 1 byte/sample instead of being widened into a `Vec<u16>` copy.
#[derive(Clone, Copy)]
pub(crate) enum SampleRow<'a> {
    U8(&'a [u8]),
    U16(&'a [u16]),
}

#[derive(Clone, Copy)]
struct PlaneRef<'a> {
    base: *const u8,
    stride: isize,
    width: usize,
    height: usize,
    depth: u8,
    _borrow: PhantomData<&'a ()>,
}

impl<'a> PlaneRef<'a> {
    fn row(self, y: usize) -> SampleRow<'a> {
        assert!(
            y < self.height,
            "plane row {y} out of bounds {}",
            self.height
        );
        // SAFETY: `PlaneRef<'a>` is only constructed from a live `Dav1dPicture` (or packed
        // test slice) that outlives `'a`, with `height` rows `stride` bytes apart and
        // `width` aligned samples per row.
        unsafe {
            let start = self.base.offset(y as isize * self.stride);
            if self.depth == 8 {
                SampleRow::U8(std::slice::from_raw_parts(start, self.width))
            } else {
                SampleRow::U16(std::slice::from_raw_parts(start as *const u16, self.width))
            }
        }
    }
}

/// Borrowed view of one decoded AV1 frame's planes directly inside `Dav1dPicture`, avoiding
/// an intermediate full-frame `Vec<u16>` copy before YUV-to-RGB conversion.
pub(crate) struct Frame<'a> {
    pub width: usize,
    pub height: usize,
    /// 8, 10 or 12.
    pub depth: u8,
    /// No chroma planes.
    pub mono: bool,
    /// Chroma subsampling as (x, y) shifts: (1, 1) is 4:2:0, (1, 0) 4:2:2, (0, 0) 4:4:4.
    pub shift: (u32, u32),
    /// H.273 matrix coefficients and range, as the AV1 sequence header states them.
    pub matrix: u16,
    pub full_range: bool,
    y: PlaneRef<'a>,
    u: Option<PlaneRef<'a>>,
    v: Option<PlaneRef<'a>>,
}

impl<'a> Frame<'a> {
    pub fn chroma_width(&self) -> usize {
        (self.width + (1 << self.shift.0) - 1) >> self.shift.0
    }

    #[cfg(test)]
    pub fn chroma_height(&self) -> usize {
        (self.height + (1 << self.shift.1) - 1) >> self.shift.1
    }

    pub fn y_row(&self, y: usize) -> SampleRow<'a> {
        self.y.row(y)
    }

    pub fn u_row(&self, y: usize) -> SampleRow<'a> {
        self.u.expect("non-mono frame has U plane").row(y)
    }

    pub fn v_row(&self, y: usize) -> SampleRow<'a> {
        self.v.expect("non-mono frame has V plane").row(y)
    }
}

/// One decoded AV1 frame copied into owned `Vec<u16>` planes, kept for unit tests.
#[cfg(test)]
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

#[cfg(test)]
impl Planes {
    pub fn chroma_width(&self) -> usize {
        (self.width + (1 << self.shift.0) - 1) >> self.shift.0
    }

    #[cfg(test)]
    pub fn chroma_height(&self) -> usize {
        (self.height + (1 << self.shift.1) - 1) >> self.shift.1
    }

    pub fn as_frame(&self) -> Frame<'_> {
        // Test buffers store packed `u16` samples even for 8-bit tests; pass depth 16 to
        // `PlaneRef` only for element-width selection while keeping `Frame.depth` exact.
        let make = |buf: &[u16], w: usize, h: usize| PlaneRef {
            base: buf.as_ptr() as *const u8,
            stride: (w * std::mem::size_of::<u16>()) as isize,
            width: w,
            height: h,
            depth: 16,
            _borrow: PhantomData,
        };
        let (cw, ch) = (self.chroma_width(), self.chroma_height());
        Frame {
            width: self.width,
            height: self.height,
            depth: self.depth,
            mono: self.mono,
            shift: self.shift,
            matrix: self.matrix,
            full_range: self.full_range,
            y: make(&self.y, self.width, self.height),
            u: (!self.mono).then(|| make(&self.u, cw, ch)),
            v: (!self.mono).then(|| make(&self.v, cw, ch)),
        }
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
        // of it, or - when `data_create` itself returned null - the all-zero `Dav1dData` this
        // `Data` was built from and never populated; unreferencing an emptied or still-zeroed
        // buffer is a no-op.
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

/// Decodes one AV1 item and passes a zero-copy [`Frame`] borrowing `Dav1dPicture`'s planes
/// to `f`, freeing the picture and decoder context as soon as `f` returns.
pub(crate) fn decode_with<T>(obu: &[u8], f: impl FnOnce(&Frame<'_>) -> T) -> Result<T, String> {
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
    // dav1d's default (1) returns only the operating point's first spatial layer: a
    // `--progressive`/`--layered` AVIF then decodes as its low-resolution base layer, half
    // the size the container reports. 0 is what libavif itself passes, and gets every layer
    // decoded and merged so `get_picture` returns the operating point's highest layer.
    settings.all_layers = 0;

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
    let frame = frame_view(&picture.0)?;
    Ok(f(&frame))
}

#[cfg(test)]
fn collect_plane<'a>(rows: usize, get: impl Fn(usize) -> SampleRow<'a>) -> Vec<u16> {
    let mut out = Vec::new();
    for y in 0..rows {
        match get(y) {
            SampleRow::U8(s) => out.extend(s.iter().map(|&v| u16::from(v))),
            SampleRow::U16(s) => out.extend_from_slice(s),
        }
    }
    out
}

#[cfg(test)]
pub(crate) fn decode(obu: &[u8]) -> Result<Planes, String> {
    decode_with(obu, |frame| {
        let y = collect_plane(frame.height, |row| frame.y_row(row));
        let (u, v) = if frame.mono {
            (Vec::new(), Vec::new())
        } else {
            let ch = frame.chroma_height();
            (
                collect_plane(ch, |row| frame.u_row(row)),
                collect_plane(ch, |row| frame.v_row(row)),
            )
        };
        Planes {
            width: frame.width,
            height: frame.height,
            depth: frame.depth,
            mono: frame.mono,
            shift: frame.shift,
            y,
            u,
            v,
            matrix: frame.matrix,
            full_range: frame.full_range,
        }
    })
}

fn frame_view(picture: &Dav1dPicture) -> Result<Frame<'_>, String> {
    let p = &picture.p;
    let (Ok(width), Ok(height)) = (usize::try_from(p.w), usize::try_from(p.h)) else {
        return Err(format!(
            "unusable AV1 picture: {}x{} at {} bits",
            p.w, p.h, p.bpc
        ));
    };
    let depth = p.bpc as u8;
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
    let plane_ref = |data: Option<NonNull<c_void>>,
                     stride: isize,
                     w: usize,
                     h: usize|
     -> Result<PlaneRef<'_>, String> {
        let base = data.ok_or("AV1 picture missing a plane")?.as_ptr() as *const u8;
        Ok(PlaneRef {
            base,
            stride,
            width: w,
            height: h,
            depth,
            _borrow: PhantomData,
        })
    };
    let y = plane_ref(picture.data[0], picture.stride[0], width, height)?;
    let (u, v) = if mono {
        (None, None)
    } else {
        let cw = (width + (1 << shift.0) - 1) >> shift.0;
        let ch = (height + (1 << shift.1) - 1) >> shift.1;
        (
            Some(plane_ref(picture.data[1], picture.stride[1], cw, ch)?),
            Some(plane_ref(picture.data[2], picture.stride[1], cw, ch)?),
        )
    };
    Ok(Frame {
        width,
        height,
        depth,
        mono,
        shift,
        matrix: header.mtrx as u16,
        full_range: header.color_range != 0,
        y,
        u,
        v,
    })
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
