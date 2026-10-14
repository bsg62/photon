//! The slideshow's state, on a clock that is a number: `createSlideshow`, with its
//! tests' cases.
//!
//! **The countdown belongs to a photo that is on screen, not to the clock**: it starts
//! when the viewer says the photo is `shown` and dies when it is `changed`. A plain
//! interval would count a slow decode against the photo, so a large file on a network
//! drive would flash past or be skipped altogether.
//!
//! Fullscreen is the window's to know at once - egui reports it in every frame - so the
//! Svelte store's awaits and its guards for a switch that lands after the stop have no
//! successor: the show says whether to go fullscreen as it starts and whether to leave
//! it as it stops.

/// How long the pointer must rest before the viewer's controls go, in milliseconds.
pub const CHROME_IDLE_MS: f64 = 2500.0;
/// How long a photo is held when the stored interval cannot be read, in seconds.
pub const FALLBACK_INTERVAL_S: i64 = 4;

#[derive(Clone, Debug, PartialEq)]
pub struct Slideshow {
    active: bool,
    playing: bool,
    /// The photo on screen has been shown, or has failed for good: it counts.
    shown: bool,
    /// Something covers the viewer: the countdown waits.
    held: bool,
    interval_ms: f64,
    /// When the countdown ends, while one runs.
    due: Option<f64>,
    /// When the pointer last moved.
    poked: f64,
    /// Whether the window was fullscreen as the show started: stopping does not drop the
    /// user out of a fullscreen they chose themselves. None while the window did not say.
    was_fullscreen: Option<bool>,
}

impl Default for Slideshow {
    fn default() -> Self {
        Self {
            active: false,
            playing: false,
            shown: false,
            held: false,
            interval_ms: FALLBACK_INTERVAL_S as f64 * 1000.0,
            due: None,
            poked: f64::NEG_INFINITY,
            was_fullscreen: None,
        }
    }
}

impl Slideshow {
    /// The show is on, playing or paused.
    pub fn active(&self) -> bool {
        self.active
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    /// The pointer has rested long enough that the controls should be out of the way.
    pub fn idle(&self, now: f64) -> bool {
        self.active && now - self.poked >= CHROME_IDLE_MS
    }

    fn arm(&mut self, now: f64) {
        self.due = (self.active && self.playing && self.shown && !self.held)
            .then_some(now + self.interval_ms);
    }

    /// Starts the show. `shown`: the photo on screen has its picture already, which is
    /// the usual case - the show starts from a photo the user is looking at. `fullscreen`:
    /// whether the window is, as it says. Answers whether to ask it to go fullscreen.
    pub fn start(&mut self, now: f64, shown: bool, fullscreen: Option<bool>) -> bool {
        if self.active {
            return false;
        }
        self.active = true;
        self.playing = true;
        self.shown = shown;
        self.interval_ms = FALLBACK_INTERVAL_S as f64 * 1000.0;
        self.was_fullscreen = fullscreen;
        self.poke(now);
        self.arm(now);
        fullscreen == Some(false)
    }

    /// The stored interval has been read, in seconds, for the show that is running: a
    /// read that failed keeps the fallback. The photo on screen is counted from now.
    pub fn interval(&mut self, seconds: Option<i64>, now: f64) {
        if !self.active {
            return;
        }
        if let Some(seconds) = seconds.filter(|seconds| *seconds > 0) {
            self.interval_ms = seconds as f64 * 1000.0;
        }
        // Counted from now, if it is counted at all.
        self.arm(now);
    }

    /// Stops the show. Answers whether to ask the window to leave fullscreen: only when
    /// the show took it there.
    pub fn stop(&mut self, now: f64) -> bool {
        if !self.active {
            return false;
        }
        self.active = false;
        self.playing = false;
        self.due = None;
        self.poke(now);
        self.was_fullscreen.take() == Some(false)
    }

    /// Paused, or playing again. A photo played again is given a whole interval.
    pub fn toggle(&mut self, now: f64) {
        if !self.active {
            return;
        }
        self.playing = !self.playing;
        if self.playing {
            self.arm(now);
        } else {
            self.due = None;
        }
    }

    /// The photo on screen is being replaced, by the countdown or by hand.
    pub fn changed(&mut self) {
        self.shown = false;
        self.due = None;
    }

    /// The photo is on screen, or has failed for good: its countdown starts now. Once:
    /// told again of the same photo, the countdown runs on.
    pub fn shown(&mut self, now: f64) {
        if self.shown {
            return;
        }
        self.shown = true;
        self.arm(now);
    }

    /// The viewer is covered, or uncovered. Uncovered, the photo on screen gets a whole
    /// interval again: the user has just been looking at the dialog, not at it.
    pub fn hold(&mut self, on: bool, now: f64) {
        if self.held == on {
            return;
        }
        self.held = on;
        if on {
            self.due = None;
        } else {
            self.arm(now);
        }
    }

    /// The pointer moved: the controls come back, and the wait begins again.
    pub fn poke(&mut self, now: f64) {
        self.poked = now;
    }

    /// Whether the countdown has ended: the show goes on to the next photo, which is
    /// `changed`. Where there is none to go to - a view of one photo - the show stays,
    /// and counts nothing more: a still window, as the Svelte show is.
    pub fn due(&mut self, now: f64) -> bool {
        if self.due.is_some_and(|due| now >= due) {
            self.due = None;
            return true;
        }
        false
    }

    /// When something next happens by itself - the countdown's end, the controls going -
    /// for a frame to be asked for then. None for a still window.
    pub fn next(&self, now: f64) -> Option<f64> {
        let idle = (self.active && !self.idle(now)).then_some(self.poked + CHROME_IDLE_MS);
        match (self.due, idle) {
            (Some(due), Some(idle)) => Some(due.min(idle)),
            (due, idle) => due.or(idle),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A show started at nought on a photo that is shown, and how many times it has
    /// advanced by `at`.
    fn started(shown: bool) -> Slideshow {
        let mut show = Slideshow::default();
        show.start(0.0, shown, Some(false));
        show
    }

    /// Whether the show advances at `at`: when it does, it goes on to another photo.
    fn advances(show: &mut Slideshow, at: f64) -> bool {
        let due = show.due(at);
        if due {
            show.changed();
        }
        due
    }

    #[test]
    fn it_advances_one_interval_after_the_photo_is_shown_not_after_the_last_advance() {
        let mut show = started(true);
        assert!(!advances(&mut show, 3999.0));
        assert!(advances(&mut show, 4000.0));
        // The next photo takes ten seconds to decode: the countdown has not started.
        assert!(!advances(&mut show, 14_000.0));
        show.shown(14_000.0);
        assert!(!advances(&mut show, 17_999.0));
        assert!(advances(&mut show, 18_000.0));
    }

    #[test]
    fn it_does_not_advance_while_held_and_gives_the_photo_a_whole_interval_once_released() {
        let mut show = started(true);
        show.hold(true, 3000.0);
        assert!(!advances(&mut show, 63_000.0));
        // A photo that finishes loading under the dialog is held too.
        show.changed();
        show.shown(63_000.0);
        assert!(!advances(&mut show, 123_000.0));
        assert!(show.playing());
        show.hold(false, 123_000.0);
        assert!(!advances(&mut show, 126_999.0));
        assert!(advances(&mut show, 127_000.0));
    }

    #[test]
    fn it_does_not_count_a_photo_that_is_still_loading_on_start_or_on_resume() {
        let mut show = started(false);
        assert!(!advances(&mut show, 60_000.0));
        show.toggle(60_000.0);
        show.toggle(60_000.0);
        assert!(!advances(&mut show, 120_000.0));
        show.shown(120_000.0);
        assert!(advances(&mut show, 124_000.0));
    }

    #[test]
    fn it_uses_the_stored_interval_counted_from_its_answer() {
        let mut show = started(true);
        show.interval(Some(9), 10.0);
        assert!(!advances(&mut show, 9009.0));
        assert!(advances(&mut show, 9010.0));
    }

    #[test]
    fn a_failed_or_senseless_read_keeps_four_seconds() {
        for read in [None, Some(0), Some(-3)] {
            let mut show = started(true);
            show.interval(read, 0.0);
            assert!(!advances(&mut show, 3999.0), "{read:?}");
            assert!(advances(&mut show, 4000.0), "{read:?}");
        }
    }

    // A read that lands on a photo still loading starts no countdown, and one for a show
    // that has stopped changes nothing.
    #[test]
    fn the_interval_read_starts_nothing_of_its_own() {
        let mut show = started(false);
        show.interval(Some(2), 0.0);
        assert!(!advances(&mut show, 60_000.0));
        show.shown(60_000.0);
        assert!(advances(&mut show, 62_000.0));
        let mut show = started(true);
        show.stop(1.0);
        show.interval(Some(2), 1.0);
        assert!(!advances(&mut show, 60_000.0));
    }

    #[test]
    fn it_holds_while_paused_and_gives_a_resumed_photo_a_full_interval() {
        let mut show = started(true);
        show.toggle(3000.0);
        assert!(!show.playing());
        assert!(!advances(&mut show, 63_000.0));
        show.toggle(63_000.0);
        assert!(!advances(&mut show, 66_999.0));
        assert!(advances(&mut show, 67_000.0));
    }

    #[test]
    fn it_restarts_the_countdown_when_the_user_steps_by_hand() {
        let mut show = started(true);
        show.changed();
        show.shown(3000.0);
        assert!(!advances(&mut show, 6999.0));
        assert!(advances(&mut show, 7000.0));
    }

    // Told twice that a photo is shown - its preview and then its full picture, a window
    // resized - the countdown runs on from the first.
    #[test]
    fn a_photo_told_shown_again_is_not_counted_again() {
        let mut show = started(true);
        show.shown(3000.0);
        assert!(advances(&mut show, 4000.0));
    }

    // A view of one photo has nothing to go on to: the photo stays, told shown in every
    // frame, and nothing is counted again.
    #[test]
    fn a_show_with_nowhere_to_go_stays_and_counts_nothing_more() {
        let mut show = started(true);
        assert!(show.due(4000.0));
        show.shown(4000.0);
        assert!(!show.due(60_000.0));
        assert_eq!(show.next(CHROME_IDLE_MS * 2.0), None);
    }

    #[test]
    fn it_never_advances_after_stop() {
        let mut show = started(true);
        show.stop(0.0);
        show.shown(0.0);
        assert!(!advances(&mut show, 60_000.0));
        assert!(!show.active());
    }

    #[test]
    fn it_goes_fullscreen_for_the_show_and_leaves_it_afterwards() {
        let mut show = Slideshow::default();
        assert!(show.start(0.0, true, Some(false)));
        assert!(show.stop(1.0));
        assert!(!show.stop(2.0), "once");
    }

    #[test]
    fn it_leaves_alone_a_fullscreen_the_user_was_already_in() {
        let mut show = Slideshow::default();
        assert!(!show.start(0.0, true, Some(true)));
        assert!(!show.stop(1.0));
    }

    // A window that does not say whether it is fullscreen is asked for nothing, and the
    // show plays on in it.
    #[test]
    fn it_plays_on_in_a_window_that_does_not_say() {
        let mut show = Slideshow::default();
        assert!(!show.start(0.0, true, None));
        assert!(advances(&mut show, 4000.0));
        assert!(!show.stop(4000.0));
    }

    #[test]
    fn a_second_start_does_nothing() {
        let mut show = started(true);
        assert!(!show.start(1000.0, true, Some(false)));
        assert!(advances(&mut show, 4000.0));
    }

    #[test]
    fn the_controls_go_once_the_pointer_rests_and_only_during_a_show() {
        let mut show = Slideshow::default();
        show.poke(0.0);
        assert!(!show.idle(CHROME_IDLE_MS * 2.0));
        show.start(10_000.0, true, Some(false));
        assert!(!show.idle(10_000.0 + CHROME_IDLE_MS - 1.0));
        show.poke(12_000.0);
        assert!(!show.idle(12_000.0 + CHROME_IDLE_MS - 1.0));
        assert!(show.idle(12_000.0 + CHROME_IDLE_MS));
        show.stop(20_000.0);
        assert!(!show.idle(30_000.0));
    }

    // A frame is asked for when the countdown ends and when the controls go, and for
    // nothing else: between two photos a show is a still window.
    #[test]
    fn the_next_frame_is_the_countdowns_end_or_the_controls_going() {
        let mut show = Slideshow::default();
        assert_eq!(show.next(0.0), None);
        show.start(0.0, true, Some(false));
        assert_eq!(show.next(0.0), Some(CHROME_IDLE_MS));
        assert_eq!(show.next(CHROME_IDLE_MS), Some(4000.0));
        show.toggle(CHROME_IDLE_MS);
        assert_eq!(show.next(CHROME_IDLE_MS), None, "paused and idle: still");
        show.toggle(3000.0);
        show.poke(5000.0);
        assert_eq!(show.next(5000.0), Some(7000.0));
        show.stop(5000.0);
        assert_eq!(show.next(5000.0), None);
    }
}
