//! The posters of videos, made by the window (spec `2026-10-14-photon-native-video-design.md`,
//! "Posters, made by the window"): the engine keeps the list of videos waiting for one and
//! a thread here makes them, one at a time - a media pipeline is heavy - through the
//! system's own framework (`video::poster`). The list, its claims and the crash-loop guard
//! are the engine's, built for the web view's thumbnailer (`video-thumbnailer.svelte.ts`)
//! and used as they stand; what a poster's failure is reported as is that file's rule,
//! ported whole (`answer`).

use crate::video::{self, Failure, Picture};
use photon_core::thumbs::VideoFailure;
use photon_engine::{commands, engine::Engine};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// What makes a poster: the system's framework, or a test's stand-in for it.
pub trait Posters: Send + Sync + 'static {
    /// Whether anything can be decoded here at all: asked once, as the worker starts.
    fn can_play(&self) -> bool;
    /// The frame at `at` seconds of the video at `path`, turned as it is seen.
    fn poster(&self, path: &Path, at: f64) -> Result<Picture, Failure>;
    /// How long a poster is waited for before it is a failure: `JOB_TIMEOUT`, but in a
    /// test that cannot wait that long.
    fn patience(&self) -> Duration {
        JOB_TIMEOUT
    }
}

/// The system's own framework.
pub struct System;

impl Posters for System {
    fn can_play(&self) -> bool {
        video::can_play()
    }

    fn poster(&self, path: &Path, at: f64) -> Result<Picture, Failure> {
        video::poster(path, at)
    }
}

/// A poster that has not come in this long is a failure, not a slow disk: the web view's
/// rule, whose spike's 4K file gave its frame in 200-300 ms.
pub const JOB_TIMEOUT: Duration = Duration::from_secs(15);
/// How long one ask for a job waits: the engine long-polls, and a worker told to stop
/// hears it at the next ask - which is what the window's closing waits for.
const ASK: Duration = Duration::from_millis(250);
/// After an ask that failed: an engine that answers at once must not be asked in a loop.
const IDLE: Duration = Duration::from_secs(1);
/// How often a poster being made is looked at for a stop or its time running out.
const LOOK: Duration = Duration::from_millis(50);
/// The longest side of a poster handed over: the preview's, which is the largest picture
/// the engine makes of it.
pub const POSTER_EDGE: u32 = 1600;

/// The thread that makes posters, for as long as it lives.
pub struct PosterWorker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl PosterWorker {
    /// Opens the engine's session of videos - supported or not, as `posters` says; not
    /// supported, the engine answers every video's tile as having no poster yet, and
    /// nothing is asked of the system - and makes posters until told to stop.
    pub fn start(engine: Arc<Engine>, posters: Arc<dyn Posters>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = thread::Builder::new()
            .name("photon-posters".into())
            .spawn(move || run(&engine, posters, &stopping))
            .map_err(|err| tracing::warn!("no thread to make videos' posters: {err}"))
            .ok();
        PosterWorker { stop, thread }
    }

    /// Tells the thread to stop, and waits for it up to `patience`: whether it stopped.
    /// A poster being made when it is told is answered as not made this session, and the
    /// thread making it is left to finish on its own - a decoder cannot be interrupted.
    pub fn finish(&mut self, patience: Duration) -> bool {
        self.stop.store(true, Ordering::SeqCst);
        let Some(thread) = self.thread.take() else {
            return true;
        };
        let until = Instant::now() + patience;
        while !thread.is_finished() {
            if Instant::now() > until {
                return false;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let _ = thread.join();
        true
    }
}

impl Drop for PosterWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn run(engine: &Arc<Engine>, posters: Arc<dyn Posters>, stop: &AtomicBool) {
    let supported = posters.can_play();
    if let Err(err) = commands::video_session_start(engine, supported) {
        tracing::warn!("the videos' session did not start: {err}");
        return;
    }
    // A session that cannot play is handed no job - the engine keeps its videos off the
    // list - so asking would only be a thread waking four times a second for nothing.
    if !supported {
        return;
    }
    // A decoder left behind at its patience, still at work.
    let mut behind: Option<JoinHandle<()>> = None;
    while !stop.load(Ordering::SeqCst) {
        // **One poster at a time, left-behind decoders included**: a media pipeline is
        // heavy, and a source reader on a share that has stopped answering never returns
        // - each video timed out beside the last would have left one more, with its file
        // open, for the rest of the session. A decoder that never comes back makes no
        // more posters this session, which is what a share that answers nothing allows.
        if behind.as_ref().is_some_and(|thread| !thread.is_finished()) {
            thread::sleep(LOOK);
            continue;
        }
        behind = None;
        match commands::next_video_job(engine, ASK) {
            Ok(Some(job)) => behind = draw(engine, &posters, stop, job.id, &job.key),
            Ok(None) => {}
            Err(err) => {
                tracing::warn!("no video to make a poster of: {err}");
                rest(IDLE, stop);
            }
        }
    }
}

/// `long`, or until told to stop.
fn rest(long: Duration, stop: &AtomicBool) {
    let until = Instant::now() + long;
    while Instant::now() < until && !stop.load(Ordering::SeqCst) {
        thread::sleep(LOOK);
    }
}

/// What came of asking for a poster.
#[derive(Debug)]
pub enum Drawn {
    Made(Result<Picture, Failure>),
    /// It took longer than the poster's patience.
    TimedOut,
    /// The window closed while it was being made.
    Stopped,
}

/// Makes the job's poster and answers it - every job is answered, or its claim counts at
/// the next launch as a window that died opening the video. The decoder's thread, where it
/// was left behind still at work.
fn draw(
    engine: &Arc<Engine>,
    posters: &Arc<dyn Posters>,
    stop: &AtomicBool,
    id: i64,
    key: &str,
) -> Option<JoinHandle<()>> {
    let (drawn, behind) = if stop.load(Ordering::SeqCst) {
        // Taken in the ask that was waiting as the window closed: answered without a
        // decoder started for nothing.
        (Drawn::Stopped, None)
    } else {
        match engine.lib.item(id) {
            Ok(Some(item)) => made(
                posters,
                stop,
                PathBuf::from(item.path),
                video::poster_time(item.duration_ms),
            ),
            // Gone since it was listed: answered all the same.
            _ => (
                Drawn::Made(Err(Failure::Unsupported("no longer in the library".into()))),
                None,
            ),
        }
    };
    let answered = match answer(drawn) {
        Ok(jpeg) => commands::put_video_frame(engine, id, key, &jpeg).or_else(|err| {
            // The poster was made, so the file is not what failed: a full disk says
            // nothing of the video, and `Decode` would fail a healthy one for good.
            tracing::warn!("a video's poster was not stored: {err}");
            commands::video_frame_failed(engine, id, key, VideoFailure::Unsupported)
        }),
        Err(reason) => commands::video_frame_failed(engine, id, key, reason),
    };
    if let Err(err) = answered {
        tracing::warn!("a video's poster went unanswered: {err}");
    }
    behind
}

/// The poster asked of `posters` on a thread of its own, so that a decoder that hangs is
/// left behind at its `patience` and a stop is heard within `LOOK` - with that thread,
/// where it was left behind.
fn made(
    posters: &Arc<dyn Posters>,
    stop: &AtomicBool,
    path: PathBuf,
    at: f64,
) -> (Drawn, Option<JoinHandle<()>>) {
    let (tx, rx) = mpsc::channel();
    let asked = posters.clone();
    let spawned = thread::Builder::new()
        .name("photon-poster".into())
        .spawn(move || {
            // A decoder that panics fails this video, not the worker.
            let poster = catch_unwind(AssertUnwindSafe(|| asked.poster(&path, at)))
                .unwrap_or_else(|_| Err(Failure::Broken("the decoder panicked".into())));
            let _ = tx.send(poster);
        });
    let decoder = match spawned {
        Ok(decoder) => decoder,
        Err(err) => {
            let no = Failure::Unsupported(format!("no thread: {err}"));
            return (Drawn::Made(Err(no)), None);
        }
    };
    let until = Instant::now() + posters.patience();
    loop {
        match rx.recv_timeout(LOOK) {
            Ok(poster) => return (Drawn::Made(poster), None),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let gone = Failure::Broken("the decoder went away".into());
                return (Drawn::Made(Err(gone)), None);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if stop.load(Ordering::SeqCst) {
                    return (Drawn::Stopped, Some(decoder));
                }
                if Instant::now() > until {
                    return (Drawn::TimedOut, Some(decoder));
                }
            }
        }
    }
}

/// What the engine is told of a poster: the JPEG to store, or why there is none.
///
/// - `Unsupported` is the system's answer (no decoder) and is not held against the file:
///   the video stays waiting, for a session that can. So is a stop - the window closing
///   on a healthy video - and a picture with no size, which is a file with no picture the
///   system can decode (an audio-only `.mp4`, an HEVC one with its sound decodable).
/// - `Decode` is the file's: it failed to decode. `Timeout`, that it took too long.
pub fn answer(drawn: Drawn) -> Result<Vec<u8>, VideoFailure> {
    match drawn {
        Drawn::Made(Ok(picture)) if picture.width == 0 || picture.height == 0 => {
            Err(VideoFailure::Unsupported)
        }
        Drawn::Made(Ok(picture)) => jpeg(&picture).ok_or(VideoFailure::Unsupported),
        Drawn::Made(Err(Failure::Unsupported(_))) | Drawn::Stopped => {
            Err(VideoFailure::Unsupported)
        }
        Drawn::Made(Err(Failure::Broken(_))) => Err(VideoFailure::Decode),
        Drawn::TimedOut => Err(VideoFailure::Timeout),
    }
}

/// `picture` fitted within `POSTER_EDGE` and encoded as a JPEG.
fn jpeg(picture: &Picture) -> Option<Vec<u8>> {
    let rgb: Vec<u8> = (picture.rgba.as_chunks::<4>().0.iter())
        .flat_map(|px| [px[0], px[1], px[2]])
        .collect();
    let mut image = image::RgbImage::from_raw(picture.width, picture.height, rgb)?;
    let longest = picture.width.max(picture.height);
    if longest > POSTER_EDGE {
        let scale = f64::from(POSTER_EDGE) / f64::from(longest);
        let side = |n: u32| ((f64::from(n) * scale).round() as u32).max(1);
        image = image::imageops::resize(
            &image,
            side(picture.width),
            side(picture.height),
            image::imageops::FilterType::Triangle,
        );
    }
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .encode_image(&image)
        .ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32) -> Picture {
        Picture {
            width,
            height,
            at: 0.4,
            rgba: [200, 30, 30, 255].repeat((width * height) as usize),
        }
    }

    fn decoded(jpeg: &[u8]) -> image::RgbImage {
        image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)
            .unwrap()
            .to_rgb8()
    }

    #[test]
    fn a_poster_is_a_jpeg_of_its_picture() {
        let jpeg = answer(Drawn::Made(Ok(solid(64, 36)))).unwrap();
        let image = decoded(&jpeg);
        assert_eq!(image.dimensions(), (64, 36));
        let [r, g, b] = image.get_pixel(32, 18).0;
        assert!(r > 180 && g < 60 && b < 60, "{:?}", (r, g, b));
    }

    #[test]
    fn a_poster_larger_than_the_preview_is_fitted_within_it() {
        let wide = decoded(&answer(Drawn::Made(Ok(solid(3200, 900)))).unwrap());
        assert_eq!(wide.dimensions(), (1600, 450));
        let tall = decoded(&answer(Drawn::Made(Ok(solid(900, 3200)))).unwrap());
        assert_eq!(tall.dimensions(), (450, 1600));
        let small = decoded(&answer(Drawn::Made(Ok(solid(1600, 20)))).unwrap());
        assert_eq!(small.dimensions(), (1600, 20));
    }

    // The web view's rule (`video-thumbnailer.svelte.ts`): the system's lack and the
    // window's closing keep the video waiting; the file's failure and a poster that never
    // came are held against it.
    #[test]
    fn what_a_failure_is_reported_as() {
        let unsupported = Failure::Unsupported("no decoder".into());
        let broken = Failure::Broken("bad file".into());
        assert_eq!(
            answer(Drawn::Made(Err(unsupported))),
            Err(VideoFailure::Unsupported)
        );
        assert_eq!(answer(Drawn::Made(Err(broken))), Err(VideoFailure::Decode));
        assert_eq!(answer(Drawn::TimedOut), Err(VideoFailure::Timeout));
        assert_eq!(answer(Drawn::Stopped), Err(VideoFailure::Unsupported));
        // A picture with no size: a file whose picture the system cannot decode.
        assert_eq!(
            answer(Drawn::Made(Ok(solid(0, 0)))),
            Err(VideoFailure::Unsupported)
        );
    }
}
