//! GStreamer: `playbin`, its picture to an `appsink` as RGBA, turned as the file says by
//! `videoflip video-direction=auto`, its sound to the system's own sink. Its plugins are
//! looked for when a pipeline is made: a system without them still starts photon, and a
//! video then answers `Unsupported`, where the web view's process aborted.

use super::{Failure, Picture};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use std::{cell::Cell, path::Path, sync::OnceLock};

/// How long a pipeline is given to say it is ready, or why not.
const WAIT: gst::ClockTime = gst::ClockTime::from_seconds(10);

fn init() -> Result<(), Failure> {
    static DONE: OnceLock<Result<(), String>> = OnceLock::new();
    DONE.get_or_init(|| gst::init().map_err(|e| e.to_string()))
        .clone()
        .map_err(Failure::Unsupported)
}

/// Whether the system can play a phone's video at all: GStreamer's MP4 demuxer, an H.264
/// decoder, and the two elements every pipeline here is built of. The first two are in
/// `gst-plugins-good` and `gst-libav` (or another decoder's package), which a Linux
/// desktop does not always have.
pub fn can_play() -> bool {
    if init().is_err() {
        return false;
    }
    let h264 = gst::Caps::builder("video/x-h264").build();
    let decodes = gst::ElementFactory::factories_with_type(
        gst::ElementFactoryType::DECODER,
        gst::Rank::MARGINAL,
    )
    .iter()
    .any(|factory| factory.can_sink_any_caps(&h264));
    decodes
        && ["qtdemux", "videoflip", "videoconvert"]
            .iter()
            .all(|name| gst::ElementFactory::find(name).is_some())
}

/// A missing element or decoder is the system's lack; anything else, the file's.
fn failure(err: &gst::glib::Error, debug: Option<&str>) -> Failure {
    let said = match debug {
        Some(debug) => format!("{err} ({debug})"),
        None => err.to_string(),
    };
    if err.matches(gst::CoreError::MissingPlugin) || err.matches(gst::StreamError::CodecNotFound) {
        Failure::Unsupported(said)
    } else {
        Failure::Broken(said)
    }
}

fn element(name: &str) -> Result<gst::Element, Failure> {
    gst::ElementFactory::make(name)
        .build()
        .map_err(|_| Failure::Unsupported(format!("GStreamer has no element `{name}`")))
}

/// `playbin` over `path`, its picture to an RGBA sink, its sound to the system's sink or,
/// for a poster, to none.
fn pipeline(
    path: &Path,
    sync: bool,
    sound: bool,
) -> Result<(gst::Element, gst_app::AppSink), Failure> {
    init()?;
    let uri = gst::glib::filename_to_uri(path, None).map_err(|e| Failure::Broken(e.to_string()))?;
    let playbin = element("playbin")?;
    playbin.set_property("uri", uri.as_str());
    let sink = gst_app::AppSink::builder()
        .caps(
            &gst_video::VideoCapsBuilder::new()
                .format(gst_video::VideoFormat::Rgba)
                .build(),
        )
        .sync(sync)
        // Only the newest frame is wanted: one drawn late is one nobody sees.
        .max_buffers(1)
        .drop(true)
        .build();
    let flip = element("videoflip")?;
    flip.set_property_from_str("video-direction", "auto");
    playbin.set_property("video-filter", &flip);
    playbin.set_property("video-sink", &sink);
    if !sound {
        playbin.set_property("audio-sink", &element("fakesink")?);
    }
    Ok((playbin, sink))
}

/// Until the pipeline has finished changing state, or says why it cannot. **A video
/// whose picture has no decoder is `Unsupported` even where its sound has one**:
/// `decodebin` errs only when no stream at all can be decoded, and otherwise plays the
/// sound alone, saying what it lacks in a `missing-plugin` message and nowhere else - an
/// iPhone's HEVC on a Linux with no HEVC decoder was a poster "broken" for good, and a
/// player of sound over nothing.
fn settled(playbin: &gst::Element) -> Result<(), Failure> {
    let bus = playbin.bus().ok_or(Failure::Broken("no bus".into()))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from(WAIT);
    let mut lacking = None;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let message = bus
            .timed_pop_filtered(
                gst::ClockTime::from_nseconds(left.as_nanos() as u64),
                &[
                    gst::MessageType::AsyncDone,
                    gst::MessageType::Error,
                    gst::MessageType::Element,
                ],
            )
            .ok_or(Failure::Broken(
                "GStreamer gave no answer in ten seconds".into(),
            ))?;
        match message.view() {
            gst::MessageView::Error(err) => {
                return Err(failure(
                    &err.error(),
                    err.debug().as_ref().map(|d| d.as_str()),
                ));
            }
            gst::MessageView::Element(element) => {
                if let Some(missing) = element.structure().and_then(lacks_picture) {
                    lacking = Some(missing);
                }
            }
            _ => {
                return match lacking {
                    Some(missing) => Err(Failure::Unsupported(format!(
                        "GStreamer has no decoder for this video's picture ({missing})"
                    ))),
                    None => Ok(()),
                };
            }
        }
    }
}

/// What a `missing-plugin` message says is missing, where it is a decoder of pictures.
fn lacks_picture(said: &gst::StructureRef) -> Option<String> {
    if said.name() != "missing-plugin" || said.get::<&str>("type").ok()? != "decoder" {
        return None;
    }
    let caps = said.get::<gst::Caps>("detail").ok()?;
    let kind = caps.structure(0)?.name();
    kind.starts_with("video/").then(|| {
        said.get::<String>("name")
            .unwrap_or_else(|_| kind.to_string())
    })
}

fn set_state(playbin: &gst::Element, state: gst::State) -> Result<(), Failure> {
    match playbin.set_state(state) {
        Ok(_) => Ok(()),
        // A refused change says why on the bus.
        Err(_) => settled(playbin).and(Err(Failure::Broken("GStreamer refused".into()))),
    }
}

fn picture(sample: &gst::Sample) -> Option<Picture> {
    let info = gst_video::VideoInfo::from_caps(sample.caps()?).ok()?;
    let buffer = sample.buffer()?;
    let at = buffer.pts()?.nseconds() as f64 / 1e9;
    let map = buffer.map_readable().ok()?;
    Picture::from_rows(
        info.width(),
        info.height(),
        at,
        map.as_slice(),
        info.stride()[0] as usize,
        false,
    )
}

fn seek_to(playbin: &gst::Element, at: f64) -> Result<(), Failure> {
    playbin
        .seek_simple(
            gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
            gst::ClockTime::from_nseconds((at.max(0.0) * 1e9) as u64),
        )
        .map_err(|e| Failure::Broken(e.to_string()))
}

/// The frame at `at` seconds, turned as it is seen.
pub fn poster(path: &Path, at: f64) -> Result<Picture, Failure> {
    let (playbin, sink) = pipeline(path, false, false)?;
    let _null = Null(&playbin);
    set_state(&playbin, gst::State::Paused)?;
    settled(&playbin)?;
    seek_to(&playbin, at)?;
    settled(&playbin)?;
    let sample = sink
        .try_pull_preroll(WAIT)
        .ok_or(Failure::Broken("no frame to show".into()))?;
    picture(&sample).ok_or(Failure::Broken("a frame with no picture".into()))
}

/// A pipeline let go of: put to `Null`, which every GStreamer element must be before it
/// is dropped, or it keeps its threads and the file open.
struct Null<'a>(&'a gst::Element);

impl Drop for Null<'_> {
    fn drop(&mut self) {
        let _ = self.0.set_state(gst::State::Null);
    }
}

/// A video being played, with its sound.
pub struct Player {
    playbin: gst::Element,
    sink: gst_app::AppSink,
    ended: Cell<bool>,
    /// Whether the pipeline has prerolled - it knows its size and length - since it was
    /// opened.
    ready: Cell<bool>,
    /// What a `missing-plugin` message said the picture lacks, until the pipeline settles.
    lacking: std::cell::RefCell<Option<String>>,
    failed: std::cell::RefCell<Option<Failure>>,
    /// The time of the last frame handed over, so a frame is handed over once.
    shown: Cell<Option<u64>>,
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.playbin.set_state(gst::State::Null);
    }
}

impl Player {
    /// Opened, and on its way to being ready, paused at its start: nothing waits here -
    /// it is called on the window's thread - and what goes wrong from here on is said by
    /// `failure`.
    pub fn open(path: &Path) -> Result<Player, Failure> {
        let (playbin, sink) = pipeline(path, true, true)?;
        let player = Player {
            playbin,
            sink,
            ended: Cell::new(false),
            ready: Cell::new(false),
            lacking: Default::default(),
            failed: Default::default(),
            shown: Cell::new(None),
        };
        if player.playbin.set_state(gst::State::Paused).is_err() {
            // A refused change says why on the bus.
            player.read_bus();
            return Err(player
                .failed
                .take()
                .unwrap_or(Failure::Broken("GStreamer refused".into())));
        }
        Ok(player)
    }

    /// Whether it knows its size and length, and has its first frame: until then it
    /// shows nothing, and `play` is kept for when it is.
    pub fn ready(&self) -> bool {
        self.read_bus();
        self.ready.get() && self.failed.borrow().is_none()
    }

    pub fn play(&self) {
        let _ = self.playbin.set_state(gst::State::Playing);
    }

    pub fn pause(&self) {
        let _ = self.playbin.set_state(gst::State::Paused);
    }

    /// To `at` seconds, to the frame: a flushing seek, which leaves the player playing or
    /// paused as it was.
    pub fn seek(&self, at: f64) {
        // An end already on the bus is the end before the seek: read now, it is not
        // taken for the end after it.
        self.read_bus();
        self.ended.set(false);
        let _ = seek_to(&self.playbin, at);
    }

    /// From nought to one.
    pub fn set_volume(&self, volume: f64) {
        self.playbin.set_property("volume", volume.clamp(0.0, 1.0));
    }

    pub fn set_muted(&self, muted: bool) {
        self.playbin.set_property("mute", muted);
    }

    /// Seconds into the video, as the player has it.
    pub fn position(&self) -> Option<f64> {
        let at = self.playbin.query_position::<gst::ClockTime>()?;
        Some(at.nseconds() as f64 / 1e9)
    }

    pub fn duration(&self) -> Option<f64> {
        let whole = self.playbin.query_duration::<gst::ClockTime>()?;
        Some(whole.nseconds() as f64 / 1e9)
    }

    /// Whether it has played to its end.
    pub fn ended(&self) -> bool {
        self.read_bus();
        self.ended.get()
    }

    /// What went wrong while it played, if anything did.
    pub fn failure(&self) -> Option<Failure> {
        self.read_bus();
        self.failed.borrow().clone()
    }

    /// The newest frame due, once: none when no new one has come since the last asked.
    /// While paused that is the frame the pipeline stands on - the first at opening, the
    /// one a seek landed on - which the sink holds as its preroll and hands over only when
    /// asked for so: `try_pull_sample` answers in `Playing` alone, and **throws the preroll
    /// away before it looks** (`gst_app_sink_try_pull_object`), so it is not asked first.
    /// The preroll is the first sample once playing too, so a frame is told from the last
    /// by its time.
    pub fn frame(&self) -> Option<Picture> {
        let sample = if self.playbin.current_state() == gst::State::Paused {
            self.sink.try_pull_preroll(gst::ClockTime::ZERO)
        } else {
            self.sink.try_pull_sample(gst::ClockTime::ZERO)
        }?;
        let at = sample.buffer()?.pts()?.nseconds();
        if self.shown.replace(Some(at)) == Some(at) {
            return None;
        }
        picture(&sample)
    }

    fn read_bus(&self) {
        let Some(bus) = self.playbin.bus() else {
            return;
        };
        while let Some(message) = bus.pop_filtered(&[
            gst::MessageType::Eos,
            gst::MessageType::Error,
            gst::MessageType::AsyncDone,
            gst::MessageType::Element,
        ]) {
            match message.view() {
                gst::MessageView::Eos(_) => self.ended.set(true),
                // The picture's missing decoder, as `settled` reads it for a poster.
                gst::MessageView::Element(element) => {
                    if let Some(missing) = element.structure().and_then(lacks_picture) {
                        *self.lacking.borrow_mut() = Some(missing);
                    }
                }
                gst::MessageView::AsyncDone(_) => {
                    if let Some(missing) = self.lacking.take() {
                        *self.failed.borrow_mut() = Some(Failure::Unsupported(format!(
                            "GStreamer has no decoder for this video's picture ({missing})"
                        )));
                    }
                    self.ready.set(true);
                }
                gst::MessageView::Error(err) => {
                    let said = failure(&err.error(), err.debug().as_ref().map(|d| d.as_str()));
                    *self.failed.borrow_mut() = Some(said);
                }
                _ => {}
            }
        }
    }
}
