//! A video played in the viewer (spec `2026-10-14-photon-native-video-design.md`,
//! "Playback, in the viewer"): the player of the photo on screen, and what is said where a
//! video cannot be played - the web view's words (`Viewer.svelte`'s `videoIssue`).

use crate::video::{Failure, Picture, Stream};

/// Shift with an arrow moves this far; the plain arrows stay the viewer's.
pub const SEEK_STEP: f64 = 5.0;

/// What follows the user from one video to the next for as long as photon is open, and
/// is stored nowhere: a video that opened silent because of something done to another a
/// week before would read as a fault (`video-player.svelte.ts`'s `sessionPrefs`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prefs {
    pub looping: bool,
    pub muted: bool,
    /// From nought to one.
    pub volume: f64,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            looping: false,
            muted: false,
            volume: 1.0,
        }
    }
}

/// How many frames ask for a seek's frame before it is taken not to be coming: about half
/// a second at the 8 ms they are asked at.
const AWAIT_LIMIT: u32 = 60;

/// What the user does to a video: by its strip, its picture or its keys.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act {
    /// Play, or pause - Space, the button, a click on the picture.
    Toggle,
    /// Shift with an arrow.
    SeekBy(f64),
    /// A drag along the position bar, at a fraction of the video's length: begun, moved,
    /// let go of, and called off by Escape - which puts the video back where it began.
    ScrubBegin(f64),
    ScrubTo(f64),
    ScrubEnd,
    ScrubAbandon,
    Mute,
    Volume(f64),
    Loop,
}

/// What the strip shows of the video.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strip {
    pub playing: bool,
    /// Seconds into it.
    pub time: f64,
    /// Its length; none while it is not known, when the position bar takes no press.
    pub duration: Option<f64>,
    pub prefs: Prefs,
    pub scrubbing: bool,
}

/// A drag along the position bar: where it began, and whether the video played then - it
/// is paused while dragged, so that each place shows its frame, and plays on when let go.
#[derive(Clone, Copy, Debug)]
struct Scrub {
    from: f64,
    resume: bool,
}

/// The player of one picture of one video, from the viewer's opening it until it leaves it.
/// It plays at once, with its sound, as the web view's did: told before it is ready, the
/// player keeps it for when it is.
pub struct Playing {
    id: i64,
    key: u64,
    /// Let go of as it fails: a video whose picture has no decoder plays its sound alone
    /// on Linux (`decodebin`), and kept, it played on under what is said of it.
    stream: Option<Box<dyn Stream>>,
    failure: Option<Failure>,
    prefs: Prefs,
    paused: bool,
    scrub: Option<Scrub>,
    /// A seek's frame is on its way: frames are asked for, paused or not, until it comes -
    /// or the video is at its end, or `AWAIT_LIMIT` frames have asked in vain: a seek to
    /// the end brings no frame, nor does one to the frame already shown, which a player
    /// hands over once, and waited for, they kept a still window drawing for good.
    awaiting: Option<u32>,
    /// One frame more is to be asked for, after the one that found it at its end: its
    /// last frame can come after it.
    one_more: bool,
}

impl Playing {
    /// Its sound set as the session has it before it plays: set after, a muted session
    /// would have heard the first moment of every video.
    pub fn start(id: i64, key: u64, stream: Box<dyn Stream>, prefs: Prefs) -> Self {
        stream.set_volume(prefs.volume);
        stream.set_muted(prefs.muted);
        stream.play();
        Playing {
            id,
            key,
            stream: Some(stream),
            failure: None,
            prefs,
            paused: false,
            scrub: None,
            awaiting: None,
            one_more: false,
        }
    }

    /// Whether this plays that picture of that video: another - a step, the photo turned
    /// into another file - is another player.
    pub fn is_of(&self, id: i64, key: u64) -> bool {
        (self.id, self.key) == (id, key)
    }

    /// The newest frame since the last asked, if one has come; none once it has failed,
    /// and the failure is kept. A video at its end that loops goes round from here.
    pub fn frame(&mut self) -> Option<Picture> {
        let stream = self.stream.as_ref()?;
        if let Some(failure) = stream.failure() {
            self.failure = Some(failure);
            self.stream = None;
            return None;
        }
        let frame = stream.frame();
        self.awaiting = match self.awaiting {
            _ if frame.is_some() || stream.ended() => None,
            Some(asked) if asked < AWAIT_LIMIT => Some(asked + 1),
            _ => None,
        };
        // At its end a video loops, or stands paused there - as a web view's does: a seek
        // back from the end then shows its place and plays nothing, and the button's
        // first press plays it again. Taken for playing still, the strip said "Pause"
        // over a still video, and the window drew on.
        if stream.ended() && self.scrub.is_none() && !self.paused {
            if self.prefs.looping {
                stream.seek(0.0);
                stream.play();
            } else {
                stream.pause();
                self.paused = true;
                self.one_more = true;
            }
        }
        frame
    }

    pub fn failure(&self) -> Option<&Failure> {
        self.failure.as_ref()
    }

    /// Whether frames may come still: frames are asked for while it is - while it plays,
    /// while a seek's frame is on its way, while it is dragged, and once more after its
    /// end, for a last frame that came after the frame that heard it. Not while it stands
    /// paused: a still window draws nothing.
    pub fn running(&mut self) -> bool {
        let Some(stream) = self.stream.as_ref() else {
            return false;
        };
        if self.awaiting.is_some() || self.scrub.is_some() || std::mem::take(&mut self.one_more) {
            return true;
        }
        !self.paused && !stream.ended()
    }

    /// The session's prefs as this player has them, after what the user did.
    pub fn prefs(&self) -> Prefs {
        self.prefs
    }

    /// What the strip shows; none for a player that has failed.
    pub fn strip(&self) -> Option<Strip> {
        let stream = self.stream.as_ref()?;
        Some(Strip {
            playing: !self.paused && !stream.ended(),
            time: stream.position().unwrap_or(0.0),
            // A length of nought is none: the bar would take drags that go nowhere, and
            // the place along it would be no number.
            duration: stream
                .duration()
                .filter(|whole| whole.is_finite() && *whole > 0.0),
            prefs: self.prefs,
            scrubbing: self.scrub.is_some(),
        })
    }

    /// What the user did, done to the player.
    pub fn act(&mut self, act: Act) {
        let Some(stream) = self.stream.as_ref() else {
            return;
        };
        let length = stream
            .duration()
            .filter(|whole| whole.is_finite() && *whole > 0.0);
        let seek = |at: f64| stream.seek(at);
        match act {
            // Under a drag the video's playing is the drag's to give back.
            Act::Toggle if self.scrub.is_some() => {}
            Act::Toggle if self.paused || stream.ended() => {
                // Played at its end, it plays again from its start, as a web view's does.
                if stream.ended() {
                    seek(0.0);
                    self.awaiting = Some(0);
                }
                stream.play();
                self.paused = false;
            }
            Act::Toggle => {
                stream.pause();
                self.paused = true;
            }
            Act::SeekBy(by) => {
                if let Some(whole) = length {
                    let at = stream.position().unwrap_or(0.0);
                    seek((at + by).clamp(0.0, whole));
                    self.awaiting = Some(0);
                }
            }
            Act::ScrubBegin(along) => {
                if let Some(whole) = length {
                    let resume = !self.paused && !stream.ended();
                    if resume {
                        stream.pause();
                    }
                    let from = stream.position().unwrap_or(0.0);
                    self.scrub = Some(Scrub { from, resume });
                    seek(along.clamp(0.0, 1.0) * whole);
                    self.awaiting = Some(0);
                }
            }
            Act::ScrubTo(along) => {
                if let (Some(_), Some(whole)) = (self.scrub, length) {
                    seek(along.clamp(0.0, 1.0) * whole);
                    self.awaiting = Some(0);
                }
            }
            Act::ScrubEnd | Act::ScrubAbandon => {
                if let Some(scrub) = self.scrub.take() {
                    if act == Act::ScrubAbandon {
                        seek(scrub.from);
                        self.awaiting = Some(0);
                    }
                    if scrub.resume {
                        stream.play();
                    }
                }
            }
            Act::Mute => {
                self.prefs.muted = !self.prefs.muted;
                stream.set_muted(self.prefs.muted);
            }
            Act::Volume(volume) => {
                self.prefs.volume = volume.clamp(0.0, 1.0);
                stream.set_volume(self.prefs.volume);
                // Turning the sound up is asking to hear it: a muted player whose
                // slider moves and stays silent looks broken.
                if self.prefs.muted && self.prefs.volume > 0.0 {
                    self.prefs.muted = false;
                    stream.set_muted(false);
                }
            }
            Act::Loop => self.prefs.looping = !self.prefs.looping,
        }
    }
}

/// A time as the strip says it: `m:ss`, and `h:mm:ss` from an hour on
/// (`video.ts`'s `formatDuration`).
pub fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// The web view's words: its Linux one names what to install.
pub const NO_GSTREAMER: &str = "This system can't play videos. Install GStreamer's good and \
libav plugins (see the README's video section).";
pub const CANNOT_PLAY_HERE: &str =
    "This video can't be played here. See the README's video section for details.";
pub const FAILED: &str = "This video can't be played here.";
pub const CRASHED: &str = "This video can't be opened.";

/// Why the video on screen is not being played, if it cannot be.
pub struct Why<'a> {
    /// The crash-loop guard failed it: it took the window down when it was opened, and
    /// nothing opens it again. With what the library said of it.
    pub crashed: Option<Option<&'a str>>,
    /// Whether the system can play anything at all; none while that is not known.
    pub plays: Option<bool>,
    /// What went wrong opening or playing it.
    pub failure: Option<&'a Failure>,
    /// Linux, where what is missing is GStreamer's plugins, which can be installed.
    pub linux: bool,
}

/// What is said over the poster, in the web view's order: a crash first, ahead of
/// everything; then a system that plays nothing; then the video's own failure. A failure
/// for want of a decoder on Linux is the system's too, and names what to install.
pub fn said(why: &Why<'_>) -> Option<String> {
    if let Some(error) = why.crashed {
        return Some(error.unwrap_or(CRASHED).to_owned());
    }
    let missing = |linux: bool| {
        if linux {
            NO_GSTREAMER
        } else {
            CANNOT_PLAY_HERE
        }
    };
    if why.plays == Some(false) {
        return Some(missing(why.linux).to_owned());
    }
    match why.failure? {
        Failure::Unsupported(_) if why.linux => Some(NO_GSTREAMER.to_owned()),
        _ => Some(FAILED.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Default)]
    struct Told {
        played: Cell<bool>,
        /// What the player was told, in order.
        log: std::cell::RefCell<Vec<String>>,
        at: Cell<f64>,
        dropped: Cell<bool>,
        ended: Cell<bool>,
        failure: std::cell::RefCell<Option<Failure>>,
        frames: std::cell::RefCell<Vec<Picture>>,
    }

    struct Fake(Rc<Told>);

    impl Drop for Fake {
        fn drop(&mut self) {
            self.0.dropped.set(true);
        }
    }

    impl Stream for Fake {
        fn ready(&self) -> bool {
            true
        }
        fn play(&self) {
            self.0.played.set(true);
            self.0.log.borrow_mut().push("play".into());
        }
        fn pause(&self) {
            self.0.log.borrow_mut().push("pause".into());
        }
        fn seek(&self, at: f64) {
            self.0.at.set(at);
            // Taken back from its end, a video is not at its end - but where it is sent
            // to the end itself, as a player is.
            self.0.ended.set(at >= 4.0);
            self.0.log.borrow_mut().push(format!("seek {at}"));
        }
        fn set_volume(&self, volume: f64) {
            self.0.log.borrow_mut().push(format!("volume {volume}"));
        }
        fn set_muted(&self, muted: bool) {
            self.0.log.borrow_mut().push(format!("muted {muted}"));
        }
        fn position(&self) -> Option<f64> {
            Some(self.0.at.get())
        }
        fn duration(&self) -> Option<f64> {
            Some(4.0)
        }
        fn ended(&self) -> bool {
            self.0.ended.get()
        }
        fn failure(&self) -> Option<Failure> {
            self.0.failure.borrow().clone()
        }
        fn frame(&self) -> Option<Picture> {
            self.0.frames.borrow_mut().pop()
        }
    }

    fn frame(at: f64) -> Picture {
        Picture {
            width: 1,
            height: 1,
            at,
            rgba: vec![0, 0, 0, 255],
        }
    }

    #[test]
    fn a_video_plays_as_it_opens_and_hands_over_its_frames() {
        let told = Rc::new(Told::default());
        let mut playing = Playing::start(7, 70, Box::new(Fake(told.clone())), Prefs::default());
        assert!(told.played.get());
        assert!(playing.is_of(7, 70) && !playing.is_of(7, 71) && !playing.is_of(8, 70));
        told.frames.borrow_mut().push(frame(0.5));
        assert_eq!(playing.frame().map(|frame| frame.at), Some(0.5));
        assert!(playing.running());
    }

    #[test]
    fn a_failed_video_hands_over_no_more_frames_and_keeps_its_failure() {
        let told = Rc::new(Told::default());
        let mut playing = Playing::start(7, 70, Box::new(Fake(told.clone())), Prefs::default());
        *told.failure.borrow_mut() = Some(Failure::Broken("bad".into()));
        told.frames.borrow_mut().push(frame(0.5));
        assert_eq!(playing.frame(), None);
        // A player that forgot its failure has failed all the same.
        *told.failure.borrow_mut() = None;
        assert_eq!(playing.frame(), None);
        assert_eq!(playing.failure(), Some(&Failure::Broken("bad".into())));
        assert!(!playing.running());
        // And its player is let go of, so that nothing of it plays on - its sound.
        assert!(told.dropped.get());
    }

    #[test]
    fn a_video_at_its_end_is_asked_for_one_frame_more() {
        let told = Rc::new(Told::default());
        let mut playing = Playing::start(7, 70, Box::new(Fake(told.clone())), Prefs::default());
        assert!(playing.running());
        told.ended.set(true);
        playing.frame();
        assert!(playing.running(), "once more, for a last frame");
        assert!(!playing.running());
        assert!(!told.dropped.get());
    }

    // At its end a video that does not loop stands paused, as a web view's does: the
    // strip says "Play", nothing is asked for, and a seek back shows its place and plays
    // nothing - neither a step of Shift with an arrow nor a drag along the bar.
    #[test]
    fn a_video_at_its_end_stands_paused_and_a_seek_back_does_not_play_it() {
        let told = Rc::new(Told::default());
        let mut playing = Playing::start(7, 70, Box::new(Fake(told.clone())), Prefs::default());
        told.ended.set(true);
        told.at.set(4.0);
        super::tests::told(&told);
        playing.frame();
        assert_eq!(super::tests::told(&told), ["pause"]);
        assert!(!playing.strip().unwrap().playing);
        playing.act(Act::SeekBy(-SEEK_STEP));
        told.frames.borrow_mut().push(frame(0.0));
        playing.frame();
        assert!(!playing.strip().unwrap().playing);
        playing.running();
        assert!(!playing.running(), "a still window draws nothing");
        playing.act(Act::ScrubBegin(0.5));
        playing.act(Act::ScrubEnd);
        assert_eq!(super::tests::told(&told), ["seek 0", "seek 2"]);
        // Its button plays it.
        playing.act(Act::Toggle);
        assert_eq!(super::tests::told(&told), ["play"]);
    }

    // A seek whose frame never comes - to the end itself, or to the frame already shown,
    // which a player hands over once - is not waited for for ever.
    #[test]
    fn a_seeks_frame_that_never_comes_is_not_waited_for_for_ever() {
        let told = Rc::new(Told::default());
        let mut playing = Playing::start(7, 70, Box::new(Fake(told.clone())), Prefs::default());
        playing.act(Act::Toggle);
        playing.act(Act::SeekBy(-SEEK_STEP));
        assert!(playing.running(), "its frame is waited for");
        for _ in 0..=AWAIT_LIMIT {
            playing.frame();
        }
        assert!(!playing.running(), "and then not");
        // At the end there is nothing to wait for at all.
        playing.act(Act::SeekBy(SEEK_STEP));
        playing.frame();
        assert!(!playing.running());
    }

    // Under a drag Space does nothing: the drag gives the video back its playing.
    #[test]
    fn under_a_drag_a_toggle_does_nothing() {
        let told = Rc::new(Told::default());
        let mut playing = Playing::start(7, 70, Box::new(Fake(told.clone())), Prefs::default());
        playing.act(Act::ScrubBegin(0.5));
        super::tests::told(&told);
        playing.act(Act::Toggle);
        playing.act(Act::ScrubEnd);
        assert_eq!(super::tests::told(&told), ["play"]);
        assert!(playing.strip().unwrap().playing);
    }

    fn why(failure: Option<&Failure>) -> Why<'_> {
        Why {
            crashed: None,
            plays: Some(true),
            failure,
            linux: false,
        }
    }

    #[test]
    fn what_is_said_where_a_video_cannot_be_played() {
        let unsupported = Failure::Unsupported("no decoder".into());
        let broken = Failure::Broken("bad".into());
        assert_eq!(said(&why(None)), None);
        assert_eq!(said(&why(Some(&broken))).as_deref(), Some(FAILED));
        assert_eq!(said(&why(Some(&unsupported))).as_deref(), Some(FAILED));
        // On Linux the want of a decoder names what to install.
        let linux = Why {
            linux: true,
            ..why(Some(&unsupported))
        };
        assert_eq!(said(&linux).as_deref(), Some(NO_GSTREAMER));
        let linux_broken = Why {
            linux: true,
            ..why(Some(&broken))
        };
        assert_eq!(said(&linux_broken).as_deref(), Some(FAILED));
        // A system that plays nothing, whatever the video did.
        let none = Why {
            plays: Some(false),
            ..why(None)
        };
        assert_eq!(said(&none).as_deref(), Some(CANNOT_PLAY_HERE));
        assert_eq!(
            said(&Why {
                linux: true,
                ..none
            })
            .as_deref(),
            Some(NO_GSTREAMER)
        );
        // Not known yet: nothing is said.
        assert_eq!(
            said(&Why {
                plays: None,
                ..why(None)
            }),
            None
        );
        // A crash ahead of everything, in the library's words where it has some.
        let crashed = Why {
            crashed: Some(Some("photon's window stopped while opening this video")),
            plays: Some(false),
            failure: Some(&broken),
            linux: true,
        };
        assert_eq!(
            said(&crashed).as_deref(),
            Some("photon's window stopped while opening this video")
        );
        assert_eq!(
            said(&Why {
                crashed: Some(None),
                ..crashed
            })
            .as_deref(),
            Some(CRASHED)
        );
    }

    fn told(playing: &Rc<Told>) -> Vec<String> {
        std::mem::take(&mut playing.log.borrow_mut())
    }

    // A muted session hears nothing of a video: its sound is set before it plays.
    #[test]
    fn a_video_starts_with_the_sessions_sound() {
        let told = Rc::new(Told::default());
        let prefs = Prefs {
            looping: true,
            muted: true,
            volume: 0.3,
        };
        let playing = Playing::start(7, 70, Box::new(Fake(told.clone())), prefs);
        assert_eq!(
            super::tests::told(&told),
            ["volume 0.3", "muted true", "play"]
        );
        assert_eq!(playing.prefs(), prefs);
    }

    #[test]
    fn the_controls_play_pause_seek_and_carry_their_sound() {
        let told = Rc::new(Told::default());
        let mut playing = Playing::start(7, 70, Box::new(Fake(told.clone())), Prefs::default());
        super::tests::told(&told);
        playing.act(Act::Toggle);
        assert!(
            !playing.strip().unwrap().playing && !playing.running(),
            "paused, still"
        );
        playing.act(Act::Toggle);
        assert_eq!(super::tests::told(&told), ["pause", "play"]);
        // Five seconds on, within the length; and back past the start.
        told.at.set(1.0);
        playing.act(Act::SeekBy(SEEK_STEP));
        playing.act(Act::SeekBy(-SEEK_STEP));
        assert_eq!(super::tests::told(&told), ["seek 4", "seek 0"]);
        // The sound, kept for the next video; turning it up unmutes.
        playing.act(Act::Mute);
        playing.act(Act::Volume(0.5));
        playing.act(Act::Loop);
        assert_eq!(
            super::tests::told(&told),
            ["muted true", "volume 0.5", "muted false"]
        );
        assert_eq!(
            playing.prefs(),
            Prefs {
                looping: true,
                muted: false,
                volume: 0.5
            }
        );
    }

    // A drag pauses the video, shows each place's frame, and plays on when let go of;
    // Escape puts it back where it began.
    #[test]
    fn a_drag_along_the_bar_seeks_and_escape_puts_it_back() {
        let told = Rc::new(Told::default());
        let mut playing = Playing::start(7, 70, Box::new(Fake(told.clone())), Prefs::default());
        told.at.set(1.0);
        super::tests::told(&told);
        playing.act(Act::ScrubBegin(0.5));
        playing.act(Act::ScrubTo(0.75));
        assert!(playing.strip().unwrap().scrubbing && playing.running());
        playing.act(Act::ScrubEnd);
        assert_eq!(
            super::tests::told(&told),
            ["pause", "seek 2", "seek 3", "play"]
        );
        playing.act(Act::ScrubBegin(0.25));
        playing.act(Act::ScrubAbandon);
        assert_eq!(
            super::tests::told(&told),
            ["pause", "seek 1", "seek 3", "play"]
        );
        // Dragged while paused, it stays paused.
        playing.act(Act::Toggle);
        playing.act(Act::ScrubBegin(0.5));
        playing.act(Act::ScrubEnd);
        assert_eq!(super::tests::told(&told), ["pause", "seek 2"]);
    }

    // At its end a video that loops goes round; one that does not stands, and its button
    // plays it again from the start.
    #[test]
    fn a_video_at_its_end_loops_or_plays_again_from_its_start() {
        let told = Rc::new(Told::default());
        let mut playing = Playing::start(7, 70, Box::new(Fake(told.clone())), Prefs::default());
        told.ended.set(true);
        super::tests::told(&told);
        playing.frame();
        assert_eq!(super::tests::told(&told), ["pause"]);
        assert!(!playing.strip().unwrap().playing);
        // Turning the loop on at the end plays nothing; the button does, from the start.
        playing.act(Act::Loop);
        playing.frame();
        assert!(super::tests::told(&told).is_empty());
        told.ended.set(true);
        playing.act(Act::Toggle);
        assert_eq!(super::tests::told(&told), ["seek 0", "play"]);
        // And looping, it goes round at its end.
        told.ended.set(true);
        playing.frame();
        assert_eq!(super::tests::told(&told), ["seek 0", "play"]);
    }

    #[test]
    fn a_time_is_minutes_and_seconds_and_hours_from_an_hour_on() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(4.9), "0:04");
        assert_eq!(clock(65.0), "1:05");
        assert_eq!(clock(3600.0 + 61.0), "1:01:01");
        assert_eq!(clock(-1.0), "0:00");
    }
}
