//! Media Foundation: a poster from an `IMFSourceReader` decoding to RGB32 - turned here by
//! the turn the reader says the file asks for, which it leaves undone - and a player of the
//! Media Engine in frame-server mode, which turns what it plays and hands each frame over
//! into a WIC bitmap. Its sound to the system's own output.

use super::{Failure, Picture, turn};
use std::{
    cell::Cell,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::Imaging::{
            CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICBitmap, IWICImagingFactory,
            WICBitmapCacheOnDemand, WICBitmapLockRead, WICRect,
        },
        Media::MediaFoundation::*,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
            StructuredStorage::PROPVARIANT,
        },
    },
    core::{BSTR, GUID, HRESULT, HSTRING, Result as WinResult, implement},
};

/// Media Foundation is part of every Windows photon runs on; H.264 it always decodes, and
/// HEVC with Microsoft's extension (a file without it answers `Unsupported`).
pub fn can_play() -> bool {
    started().is_ok()
}

/// COM on this thread, and Media Foundation in this process. Each is a no-op the second
/// time it is asked for.
fn started() -> Result<(), Failure> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        MFStartup(MF_VERSION, MFSTARTUP_FULL)
            .map_err(|e| Failure::Unsupported(format!("Media Foundation: {e}")))
    }
}

/// "No suitable transform", "unsupported" and their kind: the system has no decoder for
/// it. Anything else is the file's.
fn failure(err: windows::core::Error) -> Failure {
    const UNSUPPORTED: [HRESULT; 4] = [
        MF_E_TOPO_CODEC_NOT_FOUND,
        MF_E_INVALIDMEDIATYPE,
        MF_E_UNSUPPORTED_FORMAT,
        MF_E_UNSUPPORTED_BYTESTREAM_TYPE,
    ];
    if UNSUPPORTED.contains(&err.code()) {
        Failure::Unsupported(err.to_string())
    } else {
        Failure::Broken(err.to_string())
    }
}

const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

/// The frame at `at` seconds, turned as it is seen.
pub fn poster(path: &Path, at: f64) -> Result<Picture, Failure> {
    started()?;
    unsafe {
        let mut attributes = None;
        MFCreateAttributes(&mut attributes, 1).map_err(failure)?;
        let attributes = attributes.ok_or(Failure::Broken("no attributes".into()))?;
        // The reader converts the decoder's YUV to RGB itself.
        attributes
            .SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)
            .map_err(failure)?;
        let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), &attributes)
            .map_err(failure)?;
        reader
            .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
            .map_err(failure)?;
        reader.SetStreamSelection(VIDEO, true).map_err(failure)?;
        let wanted = MFCreateMediaType().map_err(failure)?;
        wanted
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(failure)?;
        wanted
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
            .map_err(failure)?;
        // Where no decoder can give RGB32 - HEVC without the extension - it says so here.
        reader
            .SetCurrentMediaType(VIDEO, None, &wanted)
            .map_err(|e| Failure::Unsupported(e.to_string()))?;
        let current = reader.GetCurrentMediaType(VIDEO).map_err(failure)?;
        let size = current.GetUINT64(&MF_MT_FRAME_SIZE).map_err(failure)?;
        let (width, height) = ((size >> 32) as u32, size as u32);
        // Degrees the stored picture is turned from how it is seen; undone clockwise.
        let rotation = current.GetUINT32(&MF_MT_VIDEO_ROTATION).unwrap_or(0);
        let position = PROPVARIANT::from((at.max(0.0) * 1e7) as i64);
        reader
            .SetCurrentPosition(&GUID::zeroed(), &position)
            .map_err(failure)?;
        loop {
            let mut flags = 0u32;
            let mut stamp = 0i64;
            let mut sample = None;
            reader
                .ReadSample(
                    VIDEO,
                    0,
                    None,
                    Some(&mut flags),
                    Some(&mut stamp),
                    Some(&mut sample),
                )
                .map_err(failure)?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Err(Failure::Broken("the end, and no frame".into()));
            }
            let Some(sample) = sample else { continue };
            let shown = stamp as f64 / 1e7;
            // The seek lands on the key frame before; the frames up to the one asked for
            // are read through. Within half a frame of it is it.
            if shown + 0.02 < at {
                continue;
            }
            let buffer = sample.ConvertToContiguousBuffer().map_err(failure)?;
            let mut data = std::ptr::null_mut();
            let mut length = 0u32;
            buffer
                .Lock(&mut data, None, Some(&mut length))
                .map_err(failure)?;
            let bytes = std::slice::from_raw_parts(data, length as usize);
            let picture = Picture::from_rows(width, height, shown, bytes, width as usize * 4, true);
            let _ = buffer.Unlock();
            let _ = rotation;
            return Ok(turn(picture, 0));
        }
    }
}

/// The Media Engine's events: whether one said it failed, and with what.
#[implement(IMFMediaEngineNotify)]
struct Notify {
    error: Arc<AtomicU32>,
    ended: Arc<AtomicU32>,
}

impl IMFMediaEngineNotify_Impl for Notify_Impl {
    fn EventNotify(&self, event: u32, _param1: usize, param2: u32) -> WinResult<()> {
        if event == MF_MEDIA_ENGINE_EVENT_ERROR.0 as u32 {
            self.error.store(param2.max(1), Ordering::SeqCst);
        }
        if event == MF_MEDIA_ENGINE_EVENT_ENDED.0 as u32 {
            self.ended.store(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

/// A video being played, with its sound.
pub struct Player {
    engine: IMFMediaEngine,
    error: Arc<AtomicU32>,
    ended: Arc<AtomicU32>,
    wic: IWICImagingFactory,
    bitmap: std::cell::RefCell<Option<(IWICBitmap, u32, u32)>>,
    stamp: Cell<i64>,
}

impl Drop for Player {
    fn drop(&mut self) {
        unsafe {
            let _ = self.engine.Shutdown();
        }
    }
}

impl Player {
    /// Opened and ready, paused at its start.
    pub fn open(path: &Path) -> Result<Player, Failure> {
        started()?;
        unsafe {
            let factory: IMFMediaEngineClassFactory =
                CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER)
                    .map_err(failure)?;
            let error = Arc::new(AtomicU32::new(0));
            let ended = Arc::new(AtomicU32::new(0));
            let notify: IMFMediaEngineNotify = Notify {
                error: error.clone(),
                ended: ended.clone(),
            }
            .into();
            let mut attributes = None;
            MFCreateAttributes(&mut attributes, 2).map_err(failure)?;
            let attributes = attributes.ok_or(Failure::Broken("no attributes".into()))?;
            attributes
                .SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &notify)
                .map_err(failure)?;
            // DXGI_FORMAT_B8G8R8A8_UNORM: what a frame is handed over in.
            attributes
                .SetUINT32(&MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, 87)
                .map_err(failure)?;
            let engine = factory.CreateInstance(0, &attributes).map_err(failure)?;
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                    .map_err(failure)?;
            let player = Player {
                engine,
                error,
                ended,
                wic,
                bitmap: Default::default(),
                stamp: Cell::new(-1),
            };
            let url = BSTR::from(path.to_string_lossy().as_ref());
            player.engine.SetSource(&url).map_err(failure)?;
            let until = Instant::now() + Duration::from_secs(10);
            // HAVE_METADATA or more: its size and length are known.
            while player.engine.GetReadyState() < 1 {
                if let Some(failure) = player.failure() {
                    return Err(failure);
                }
                if Instant::now() > until {
                    return Err(Failure::Broken("Media Foundation was never ready".into()));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(player)
        }
    }

    pub fn play(&self) {
        unsafe {
            let _ = self.engine.Play();
        }
    }

    pub fn pause(&self) {}

    /// To `at` seconds, to the frame.
    pub fn seek(&self, at: f64) {
        self.ended.store(0, Ordering::SeqCst);
        unsafe {
            let _ = self.engine.SetCurrentTime(at.max(0.0));
        }
    }

    /// From nought to one.
    pub fn set_volume(&self, volume: f64) {
        unsafe {
            let _ = self.engine.SetVolume(volume.clamp(0.0, 1.0));
        }
    }

    pub fn set_muted(&self, muted: bool) {
        unsafe {
            let _ = self.engine.SetMuted(muted);
        }
    }

    /// Seconds into the video, as the player has it.
    pub fn position(&self) -> Option<f64> {
        let at = unsafe { self.engine.GetCurrentTime() };
        at.is_finite().then_some(at)
    }

    pub fn duration(&self) -> Option<f64> {
        let whole = unsafe { self.engine.GetDuration() };
        (whole.is_finite() && whole > 0.0).then_some(whole)
    }

    /// Whether it has played to its end.
    pub fn ended(&self) -> bool {
        self.ended.load(Ordering::SeqCst) != 0 || unsafe { self.engine.IsEnded().as_bool() }
    }

    /// What went wrong while it played, if anything did. The engine's code 4 is
    /// `MF_MEDIA_ENGINE_ERR_SRC_NOT_SUPPORTED`: no decoder for it.
    pub fn failure(&self) -> Option<Failure> {
        match self.error.load(Ordering::SeqCst) {
            0 => None,
            4 => Some(Failure::Unsupported(
                "the system cannot play this video".into(),
            )),
            code => Some(Failure::Broken(format!("Media Foundation failed ({code})"))),
        }
    }

    /// The newest frame due, once: none when no new one has come since the last asked.
    pub fn frame(&self) -> Option<Picture> {
        unsafe {
            // A new frame is a new stamp: the bindings read the call's S_FALSE ("none
            // new") as success. Before the first frame is due the engine answers a stamp
            // of no time at all, and a black picture with it.
            let stamp = self.engine.OnVideoStreamTick().ok()?;
            if stamp == self.stamp.get() {
                return None;
            }
            self.stamp.set(stamp);
            let (mut width, mut height) = (0u32, 0u32);
            self.engine
                .GetNativeVideoSize(Some(&mut width), Some(&mut height))
                .ok()?;
            let mut bitmap = self.bitmap.borrow_mut();
            if bitmap
                .as_ref()
                .is_none_or(|(_, w, h)| (*w, *h) != (width, height))
            {
                let made = self
                    .wic
                    .CreateBitmap(
                        width,
                        height,
                        &GUID_WICPixelFormat32bppBGRA,
                        WICBitmapCacheOnDemand,
                    )
                    .ok()?;
                *bitmap = Some((made, width, height));
            }
            let (target, _, _) = bitmap.as_ref()?;
            let all = RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            };
            self.engine
                .TransferVideoFrame(target, None, &all, None)
                .ok()?;
            let whole = WICRect {
                X: 0,
                Y: 0,
                Width: width as i32,
                Height: height as i32,
            };
            let lock = target.Lock(&whole, WICBitmapLockRead.0 as u32).ok()?;
            let stride = lock.GetStride().ok()? as usize;
            let mut size = 0u32;
            let mut data = std::ptr::null_mut();
            lock.GetDataPointer(&mut size, &mut data).ok()?;
            let bytes = std::slice::from_raw_parts(data, size as usize);
            Some(Picture::from_rows(
                width,
                height,
                stamp as f64 / 1e7,
                bytes,
                stride,
                true,
            ))
        }
    }
}
