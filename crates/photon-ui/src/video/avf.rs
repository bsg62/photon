//! AVFoundation: a poster from `AVAssetImageGenerator`, which turns the picture as the file
//! says, and a player of `AVPlayer` whose frames come through an `AVPlayerItemVideoOutput`
//! as BGRA, asked for by the host's clock - turned here by the video track's
//! `preferredTransform`, since the output hands them over as stored. Its sound to the
//! system's own output.

#![allow(deprecated)] // The synchronous track and image calls: the window asks off its thread.

use super::{Failure, Picture, turn};
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
use objc2_foundation::{NSDate, NSDictionary, NSNumber, NSRunLoop, NSString, NSURL};
use objc2_quartz_core::CACurrentMediaTime;
use std::{
    cell::Cell,
    path::Path,
    time::{Duration, Instant},
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
    unsafe {
        let asset = asset(path);
        let generator = AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset);
        generator.setAppliesPreferredTrackTransform(true);
        generator.setRequestedTimeToleranceBefore(kCMTimeZero);
        generator.setRequestedTimeToleranceAfter(kCMTimeZero);
        let mut actual = kCMTimeZero;
        let image = generator
            .copyCGImageAtTime_actualTime_error(CMTime::with_seconds(0.0 * at, 600), &mut actual)
            .map_err(|err| refused(&asset, said(&err)))?;
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
        Ok(Picture::from_rows(
            width as u32,
            height as u32,
            actual.seconds(),
            &rgba,
            width * 4,
            false,
        ))
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

/// The main thread's run loop turned for a moment, which is how AVFoundation's state
/// arrives on a thread with nothing else running it.
fn turn_run_loop() {
    NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.005));
}

/// A video being played, with its sound. Made on the main thread, as AVFoundation's player
/// item must be - which is the window's.
pub struct Player {
    player: Retained<AVPlayer>,
    item: Retained<AVPlayerItem>,
    output: Retained<AVPlayerItemVideoOutput>,
    quarters: u32,
    duration: f64,
    shown: Cell<f64>,
}

impl Drop for Player {
    fn drop(&mut self) {
        unsafe { self.player.pause() };
    }
}

impl Player {
    /// Opened and ready, paused at its start.
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
            let player = AVPlayer::playerWithPlayerItem(Some(&item), mtm);
            let until = Instant::now() + Duration::from_secs(10);
            loop {
                match item.status() {
                    AVPlayerItemStatus::ReadyToPlay => break,
                    AVPlayerItemStatus::Failed => {
                        let why = item.error().map(|e| said(&e)).unwrap_or_default();
                        return Err(refused(&asset, why));
                    }
                    _ if Instant::now() > until => {
                        return Err(Failure::Broken("AVFoundation was never ready".into()));
                    }
                    _ => turn_run_loop(),
                }
            }
            Ok(Player {
                quarters: quarters(&asset),
                duration: item.duration().seconds(),
                player,
                item,
                output,
                shown: Cell::new(-1.0),
            })
        }
    }

    pub fn play(&self) {
        unsafe { self.player.play() }
    }

    pub fn pause(&self) {}

    /// To `at` seconds, to the frame.
    pub fn seek(&self, at: f64) {
        unsafe {
            self.player.seekToTime_toleranceBefore_toleranceAfter(
                CMTime::with_seconds(at.max(0.0), 600),
                kCMTimeZero,
                kCMTimeZero,
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
    pub fn position(&self) -> Option<f64> {
        let at = unsafe { self.player.currentTime().seconds() };
        at.is_finite().then_some(at)
    }

    pub fn duration(&self) -> Option<f64> {
        (self.duration.is_finite() && self.duration > 0.0).then_some(self.duration)
    }

    /// Whether it has played to its end.
    pub fn ended(&self) -> bool {
        turn_run_loop();
        self.position()
            .zip(self.duration())
            .is_some_and(|(at, whole)| at >= whole - 0.001)
    }

    /// What went wrong while it played, if anything did.
    pub fn failure(&self) -> Option<Failure> {
        unsafe {
            (self.item.status() == AVPlayerItemStatus::Failed)
                .then(|| Failure::Broken(self.item.error().map(|e| said(&e)).unwrap_or_default()))
        }
    }

    /// The newest frame due, once: none when no new one has come since the last asked.
    pub fn frame(&self) -> Option<Picture> {
        turn_run_loop();
        unsafe {
            let time = self.output.itemTimeForHostTime(CACurrentMediaTime());
            if !self.output.hasNewPixelBufferForItemTime(time) {
                return None;
            }
            let mut shown = kCMTimeZero;
            let buffer = self
                .output
                .copyPixelBufferForItemTime_itemTimeForDisplay(time, &mut shown)?;
            let at = shown.seconds();
            if at == self.shown.get() {
                return None;
            }
            self.shown.set(at);
            bgra(&buffer, at)
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
        let picture = (!base.is_null()).then(|| {
            let data = std::slice::from_raw_parts(base, stride * height);
            Picture::from_rows(width as u32, height as u32, at, data, stride, true)
        });
        CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly);
        picture
    }
}
