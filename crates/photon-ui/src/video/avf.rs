//! AVFoundation: a poster from `AVAssetImageGenerator`, which turns the picture as the file
//! says, and a player of `AVPlayer` whose frames come through an `AVPlayerItemVideoOutput`
//! as BGRA, asked for by the host's clock - turned here by the video track's
//! `preferredTransform`, since the output hands them over as stored. Its sound to the
//! system's own output.

#![allow(deprecated)] // The synchronous track and image calls: the window asks off its thread.

use super::{Failure, Picture, turn};
use block2::RcBlock;
use objc2::runtime::Bool;
use objc2::{AllocAnyThread, MainThreadMarker, rc::Retained, runtime::AnyObject};
use objc2_av_foundation::{
    AVAsset, AVAssetImageGenerator, AVMediaTypeVideo, AVPlayer, AVPlayerItem, AVPlayerItemStatus,
    AVPlayerItemVideoOutput, AVURLAsset,
};
use objc2_core_foundation::{CFString, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapInfo, CGColorSpaceCreateDeviceRGB, CGContext, CGImage,
    CGImageAlphaInfo,
};
use objc2_core_media::{CMTime, kCMTimeZero};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVPixelBufferPixelFormatTypeKey,
    kCVPixelFormatType_32BGRA,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};
use objc2_quartz_core::CACurrentMediaTime;
use std::{
    cell::Cell,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

/// AVFoundation is part of the system, and plays H.264 and HEVC on every Mac photon runs on.
pub fn can_play() -> bool {
    true
}

fn asset(path: &Path) -> Retained<AVURLAsset> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) }
}

fn said(err: &objc2_foundation::NSError) -> String {
    err.localizedDescription().to_string()
}

/// The quarter turns clockwise the video track asks to be shown at: its
/// `preferredTransform`'s rotation, in a picture whose y runs down - a phone held upright
/// writes `(0, 1, -1, 0)`, a quarter clockwise.
fn quarters(asset: &AVAsset) -> u32 {
    unsafe {
        let Some(kind) = AVMediaTypeVideo else {
            return 0;
        };
        let tracks = asset.tracksWithMediaType(kind);
        let Some(track) = tracks.firstObject() else {
            return 0;
        };
        let t = track.preferredTransform();
        let degrees = t.b.atan2(t.a).to_degrees();
        ((degrees / 90.0).round() as i64).rem_euclid(4) as u32
    }
}

/// The frame at `at` seconds, turned as it is seen.
pub fn poster(path: &Path, at: f64) -> Result<Picture, Failure> {
    let asset = asset(path);
    still(&asset, &generator(&asset), at)
}

/// A maker of stills from `asset`, turned as the track asks and at the very time asked.
fn generator(asset: &AVAsset) -> Retained<AVAssetImageGenerator> {
    unsafe {
        let generator = AVAssetImageGenerator::assetImageGeneratorWithAsset(asset);
        generator.setAppliesPreferredTrackTransform(true);
        generator.setRequestedTimeToleranceBefore(kCMTimeZero);
        generator.setRequestedTimeToleranceAfter(kCMTimeZero);
        generator
    }
}

/// The frame at `at` seconds of `asset`, by `generator`.
fn still(asset: &AVAsset, generator: &AVAssetImageGenerator, at: f64) -> Result<Picture, Failure> {
    unsafe {
        let mut actual = kCMTimeZero;
        let image = generator
            .copyCGImageAtTime_actualTime_error(CMTime::with_seconds(at, 600), &mut actual)
            .map_err(|err| refused(asset, said(&err)))?;
        let (width, height) = (CGImage::width(Some(&image)), CGImage::height(Some(&image)));
        let mut rgba = vec![0u8; width * height * 4];
        let space =
            CGColorSpaceCreateDeviceRGB().ok_or(Failure::Broken("no colour space".into()))?;
        let context = CGBitmapContextCreate(
            rgba.as_mut_ptr().cast(),
            width,
            height,
            8,
            width * 4,
            Some(&space),
            CGImageAlphaInfo::PremultipliedLast.0 | CGBitmapInfo::ByteOrder32Big.0,
        )
        .ok_or(Failure::Broken("no bitmap context".into()))?;
        let all = CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(width as f64, height as f64),
        );
        CGContext::draw_image(Some(&context), all, Some(&image));
        drop(context);
        Picture::from_rows(
            width as u32,
            height as u32,
            actual.seconds(),
            &rgba,
            width * 4,
            false,
        )
        .ok_or(Failure::Broken("a poster of no size".into()))
    }
}

/// A file AVFoundation cannot read: unsupported where the asset says it cannot be played
/// at all (no decoder for its video), broken otherwise.
fn refused(asset: &AVAsset, why: String) -> Failure {
    if unsafe { asset.isPlayable() } {
        Failure::Broken(why)
    } else {
        Failure::Unsupported(why)
    }
}

/// A video being played, with its sound. Made on the main thread, as AVFoundation's player
/// item must be - which is the window's.
pub struct Player {
    player: Retained<AVPlayer>,
    item: Retained<AVPlayerItem>,
    output: Retained<AVPlayerItemVideoOutput>,
    asset: Retained<AVURLAsset>,
    generator: Retained<AVAssetImageGenerator>,
    /// Where a paused player stands with no frame of it handed over yet: its start at
    /// opening, where a seek landed. **The video output vends a frame only as the player
    /// renders one, which a paused player does not**: the frame it stands on is made as a
    /// still, as a poster is (on the thread that asks, the window's - tens of
    /// milliseconds, once a seek).
    still: Cell<Option<f64>>,
    /// Seeks asked for and not yet landed, and where the last one goes. **AVFoundation
    /// seeks in the background, and its time is the old one until the seek lands**: read
    /// meanwhile, a video played to its end and taken back to its start still said it had
    /// ended. A seek's completion handler counts it landed, superseded ones included.
    seeking: Arc<AtomicUsize>,
    heading: Cell<f64>,
    quarters: u32,
    shown: Cell<f64>,
}

impl Drop for Player {
    fn drop(&mut self) {
        unsafe { self.player.pause() };
    }
}

impl Player {
    /// Opened, and on its way to being ready, paused at its start: nothing waits here
    /// for the item - it is called on the window's thread - and what goes wrong from here
    /// on is said by `failure`. (`isPlayable` and the turn are read from the file's
    /// headers as it opens, which on a share is a moment.)
    pub fn open(path: &Path) -> Result<Player, Failure> {
        let mtm = MainThreadMarker::new().ok_or(Failure::Broken(
            "a player is made on the main thread".into(),
        ))?;
        unsafe {
            let asset = asset(path);
            if !asset.isPlayable() {
                return Err(Failure::Unsupported(
                    "the system cannot play this video".into(),
                ));
            }
            let item = AVPlayerItem::playerItemWithAsset(&asset, mtm);
            // kCVPixelBufferPixelFormatTypeKey is a CFString, which is an NSString to the
            // Objective-C runtime.
            let key: &NSString = &*(kCVPixelBufferPixelFormatTypeKey as *const CFString).cast();
            let bgra = NSNumber::numberWithUnsignedInt(kCVPixelFormatType_32BGRA);
            let value: &AnyObject = &bgra;
            let attributes = NSDictionary::from_slices(&[key], &[value]);
            let output = AVPlayerItemVideoOutput::initWithPixelBufferAttributes(
                AVPlayerItemVideoOutput::alloc(),
                Some(&attributes),
            );
            item.addOutput(&output);
            // Left waiting to minimise stalling, as it is made: switched off, the player
            // stopped for good at the first stall - on macOS' runner, 0.12 s into a file
            // on its own disk - where on it begins a moment late and plays through.
            let player = AVPlayer::playerWithPlayerItem(Some(&item), mtm);
            Ok(Player {
                quarters: quarters(&asset),
                generator: generator(&asset),
                asset,
                player,
                item,
                output,
                still: Cell::new(Some(0.0)),
                seeking: Arc::new(AtomicUsize::new(0)),
                heading: Cell::new(0.0),
                shown: Cell::new(-1.0),
            })
        }
    }

    pub fn play(&self) {
        self.still.set(None);
        unsafe { self.player.play() }
    }

    pub fn pause(&self) {
        unsafe { self.player.pause() }
    }

    /// To `at` seconds, to the frame.
    pub fn seek(&self, at: f64) {
        let at = at.max(0.0);
        if unsafe { self.player.rate() } == 0.0 {
            self.still.set(Some(at));
        }
        self.heading.set(at);
        self.seeking.fetch_add(1, Ordering::SeqCst);
        let seeking = self.seeking.clone();
        let landed: RcBlock<dyn Fn(Bool)> = RcBlock::new(move |_finished: Bool| {
            seeking.fetch_sub(1, Ordering::SeqCst);
        });
        unsafe {
            self.player
                .seekToTime_toleranceBefore_toleranceAfter_completionHandler(
                    CMTime::with_seconds(at, 600),
                    kCMTimeZero,
                    kCMTimeZero,
                    &landed,
                )
        }
    }

    /// From nought to one.
    pub fn set_volume(&self, volume: f64) {
        unsafe { self.player.setVolume(volume.clamp(0.0, 1.0) as f32) }
    }

    pub fn set_muted(&self, muted: bool) {
        unsafe { self.player.setMuted(muted) }
    }

    /// Seconds into the video, as the player has it.
    /// Where a seek on its way goes, until it lands.
    pub fn position(&self) -> Option<f64> {
        if self.seeking.load(Ordering::SeqCst) > 0 {
            return Some(self.heading.get());
        }
        let at = unsafe { self.player.currentTime().seconds() };
        at.is_finite().then_some(at)
    }

    /// Once it is ready: before, the item's length is not known.
    pub fn duration(&self) -> Option<f64> {
        let whole = unsafe { self.item.duration().seconds() };
        (whole.is_finite() && whole > 0.0).then_some(whole)
    }

    /// Whether the item is ready to play: its state arrives on the main thread's run
    /// loop, which in the window winit turns.
    pub fn ready(&self) -> bool {
        unsafe { self.item.status() == AVPlayerItemStatus::ReadyToPlay }
    }

    /// Whether it has played to its end.
    pub fn ended(&self) -> bool {
        self.position()
            .zip(self.duration())
            .is_some_and(|(at, whole)| at >= whole - 0.001)
    }

    /// What went wrong while it played, if anything did.
    pub fn failure(&self) -> Option<Failure> {
        unsafe {
            (self.item.status() == AVPlayerItemStatus::Failed).then(|| {
                let why = self.item.error().map(|e| said(&e)).unwrap_or_default();
                refused(&self.asset, why)
            })
        }
    }

    /// The newest frame due, once: none when no new one has come since the last asked.
    pub fn frame(&self) -> Option<Picture> {
        unsafe {
            let time = self.output.itemTimeForHostTime(CACurrentMediaTime());
            if !self.output.hasNewPixelBufferForItemTime(time) {
                let at = self.still.take()?;
                let picture = still(&self.asset, &self.generator, at).ok()?;
                self.shown.set(picture.at);
                return Some(picture);
            }
            self.still.set(None);
            let mut shown = kCMTimeZero;
            let buffer = self
                .output
                .copyPixelBufferForItemTime_itemTimeForDisplay(time, &mut shown)?;
            let at = shown.seconds();
            if at == self.shown.get() {
                return None;
            }
            self.shown.set(at);
            Some(turn(bgra(&buffer, at)?, self.quarters))
        }
    }
}

fn bgra(buffer: &CVPixelBuffer, at: f64) -> Option<Picture> {
    unsafe {
        if CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly) != 0 {
            return None;
        }
        let (width, height) = (
            CVPixelBufferGetWidth(buffer),
            CVPixelBufferGetHeight(buffer),
        );
        let stride = CVPixelBufferGetBytesPerRow(buffer);
        let base = CVPixelBufferGetBaseAddress(buffer) as *const u8;
        let picture = if base.is_null() {
            None
        } else {
            let data = std::slice::from_raw_parts(base, stride * height);
            Picture::from_rows(width as u32, height as u32, at, data, stride, true)
        };
        CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly);
        picture
    }
}
