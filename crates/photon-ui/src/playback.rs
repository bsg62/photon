//! A video played in the viewer (spec `2026-10-14-photon-native-video-design.md`,
//! "Playback, in the viewer"): the player of the photo on screen, and what is said where a
//! video cannot be played - the web view's words (`Viewer.svelte`'s `videoIssue`).

use crate::video::{Failure, Picture, Stream};

/// The player of one picture of one video, from the viewer's opening it until it leaves it.
/// It plays at once, with its sound, as the web view's did: told before it is ready, the
/// player keeps it for when it is.
pub struct Playing {
    id: i64,
    key: u64,
    stream: Box<dyn Stream>,
    failure: Option<Failure>,
}

impl Playing {
    pub fn start(id: i64, key: u64, stream: Box<dyn Stream>) -> Self {
        stream.play();
        Playing {
            id,
            key,
            stream,
            failure: None,
        }
    }

    /// Whether this plays that picture of that video: another - a step, the photo turned
    /// into another file - is another player.
    pub fn is_of(&self, id: i64, key: u64) -> bool {
        (self.id, self.key) == (id, key)
    }

    /// The newest frame since the last asked, if one has come; none once it has failed,
    /// and the failure is kept.
    pub fn frame(&mut self) -> Option<Picture> {
        if self.failure.is_none() {
            self.failure = self.stream.failure();
        }
        if self.failure.is_some() {
            return None;
        }
        self.stream.frame()
    }

    pub fn failure(&self) -> Option<&Failure> {
        self.failure.as_ref()
    }

    /// Whether frames may come still: frames are asked for while it is.
    pub fn running(&self) -> bool {
        self.failure.is_none() && !self.stream.ended()
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
        failure: std::cell::RefCell<Option<Failure>>,
        frames: std::cell::RefCell<Vec<Picture>>,
    }

    struct Fake(Rc<Told>);

    impl Stream for Fake {
        fn ready(&self) -> bool {
            true
        }
        fn play(&self) {
            self.0.played.set(true);
        }
        fn pause(&self) {}
        fn seek(&self, _: f64) {}
        fn set_volume(&self, _: f64) {}
        fn set_muted(&self, _: bool) {}
        fn position(&self) -> Option<f64> {
            None
        }
        fn duration(&self) -> Option<f64> {
            None
        }
        fn ended(&self) -> bool {
            false
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
        let mut playing = Playing::start(7, 70, Box::new(Fake(told.clone())));
        assert!(told.played.get());
        assert!(playing.is_of(7, 70) && !playing.is_of(7, 71) && !playing.is_of(8, 70));
        told.frames.borrow_mut().push(frame(0.5));
        assert_eq!(playing.frame().map(|frame| frame.at), Some(0.5));
        assert!(playing.running());
    }

    #[test]
    fn a_failed_video_hands_over_no_more_frames_and_keeps_its_failure() {
        let told = Rc::new(Told::default());
        let mut playing = Playing::start(7, 70, Box::new(Fake(told.clone())));
        *told.failure.borrow_mut() = Some(Failure::Broken("bad".into()));
        told.frames.borrow_mut().push(frame(0.5));
        assert_eq!(playing.frame(), None);
        // A player that forgot its failure has failed all the same.
        *told.failure.borrow_mut() = None;
        assert_eq!(playing.frame(), None);
        assert_eq!(playing.failure(), Some(&Failure::Broken("bad".into())));
        assert!(!playing.running());
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
}
