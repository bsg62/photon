//! What the grid's scroll is doing: still, moving continuously, or jumping. The wanted
//! range and the thumbnail deferral are sized by it (`layout::wanted_overscan`,
//! `layout::defers_thumbs`).

/// How long without a move before a scroll counts as over. Also the longest gap two moves
/// can have and still be measured against each other: the first move after a pause has
/// nothing recent to be a speed relative to.
pub const SCROLL_SETTLE_MS: f64 = 150.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Down,
    Up,
}

/// - `Still`: no move for `SCROLL_SETTLE_MS`.
/// - `Scroll`: continuous movement - each move was less than a viewport, so what the next
///   frame shows overlaps what this one showed. `speed` is the latest measured speed in
///   pixels per millisecond, 0 when there was no recent move to measure against; `peak` is
///   the fastest this run has gone in this direction, which is what the lead is sized by,
///   so a flick slowing down does not give up the thumbnails it asked for ahead of itself.
/// - `Jump`: one move of a viewport or more - End, a scrollbar drag. Nothing on screen
///   before it is on screen after it. `stream` is true when it came within
///   `SCROLL_SETTLE_MS` of the previous move - a drag, where the next jump is already on
///   its way - and false for a jump on its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    Still,
    Scroll {
        direction: Direction,
        speed: f64,
        peak: f64,
    },
    Jump {
        stream: bool,
    },
}

/// Follows the grid's position and says what kind of movement it is.
///
/// Speed is distance over the time between two samples, never distance per sample: a frame
/// is a sample, so pixels per sample would call the same gesture twice as fast on a 60Hz
/// screen as on a 120Hz one. A jump *is* measured per sample, on purpose: it is a statement
/// about two consecutive frames - whether the second shares any rows with the first - not
/// about how fast anything is moving.
#[derive(Debug)]
pub struct ScrollSpeed {
    motion: Motion,
    // The position is known across a pause - the grid stays where it was - so a jump after
    // one is still measured from it. Only the time is forgotten.
    last_top: f64,
    last_at: Option<f64>,
}

impl Default for ScrollSpeed {
    fn default() -> Self {
        Self {
            motion: Motion::Still,
            last_top: 0.0,
            last_at: None,
        }
    }
}

impl ScrollSpeed {
    pub fn motion(&self) -> Motion {
        self.motion
    }

    /// The position after a frame's input, the time in milliseconds, and the viewport's
    /// height, which is what decides whether the move was a jump. Called for every frame
    /// in which the position changed.
    pub fn sample(&mut self, top: f64, at: f64, viewport: f64) {
        let moved = top - self.last_top;
        let elapsed = self.last_at.map(|last| at - last);
        let recent = elapsed.is_some_and(|e| e <= SCROLL_SETTLE_MS);
        if moved != 0.0 {
            if moved.abs() >= viewport {
                self.motion = Motion::Jump { stream: recent };
            } else {
                let direction = if moved > 0.0 {
                    Direction::Down
                } else {
                    Direction::Up
                };
                let same = match self.motion {
                    Motion::Scroll {
                        direction: d,
                        speed,
                        peak,
                    } if d == direction => Some((speed, peak)),
                    _ => None,
                };
                // Two samples with the same time say nothing about speed; the run's own
                // speed stands until one that does.
                let speed = match elapsed {
                    Some(e) if recent && e > 0.0 => moved.abs() / e,
                    _ => same.map_or(0.0, |(speed, _)| speed),
                };
                let peak = speed.max(same.map_or(0.0, |(_, peak)| peak));
                self.motion = Motion::Scroll {
                    direction,
                    speed,
                    peak,
                };
            }
        }
        self.last_top = top;
        self.last_at = Some(at);
    }

    /// Settles the motion once `SCROLL_SETTLE_MS` have passed without a sample. Called
    /// every frame; the timer of the Svelte grid, made a question.
    pub fn tick(&mut self, now: f64) {
        if self.last_at.is_some_and(|at| now - at >= SCROLL_SETTLE_MS) {
            self.motion = Motion::Still;
            self.last_at = None;
        }
    }

    /// When `tick` will next change anything, for the frame to ask to be drawn again then:
    /// with no input nothing else would wake it.
    pub fn settles_at(&self) -> Option<f64> {
        self.last_at.map(|at| at + SCROLL_SETTLE_MS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: f64 = 800.0;

    fn scroll(direction: Direction, speed: f64, peak: f64) -> Motion {
        Motion::Scroll {
            direction,
            speed,
            peak,
        }
    }

    #[test]
    fn speed_is_measured_by_time_and_not_by_sample() {
        // The same distance per sample at 60Hz and at 240Hz: four times the speed.
        let mut slow = ScrollSpeed::default();
        let mut quick = ScrollSpeed::default();
        for i in 0..5 {
            let i = f64::from(i);
            slow.sample(i * 40.0, 1000.0 + i * 16.0, VIEWPORT);
            quick.sample(i * 40.0, 1000.0 + i * 4.0, VIEWPORT);
        }
        assert_eq!(slow.motion(), scroll(Direction::Down, 2.5, 2.5));
        assert_eq!(quick.motion(), scroll(Direction::Down, 10.0, 10.0));
    }

    #[test]
    fn a_flick_is_a_scroll_however_fast_while_each_frame_overlaps_the_last() {
        // 40px/ms at 60Hz is 640px a frame: fast, but continuous.
        let mut speed = ScrollSpeed::default();
        for i in 0..5 {
            let i = f64::from(i);
            speed.sample(10_000.0 - i * 640.0, 1000.0 + i * 16.0, VIEWPORT);
        }
        assert_eq!(speed.motion(), scroll(Direction::Up, 40.0, 40.0));
    }

    #[test]
    fn a_move_of_a_viewport_or_more_in_one_sample_is_a_jump() {
        let mut speed = ScrollSpeed::default();
        speed.sample(100.0, 1000.0, VIEWPORT);
        speed.sample(100.0 + VIEWPORT, 1016.0, VIEWPORT);
        assert_eq!(speed.motion(), Motion::Jump { stream: true });
    }

    #[test]
    fn a_jump_after_a_pause_is_a_jump_on_its_own() {
        let mut speed = ScrollSpeed::default();
        speed.sample(100.0, 1000.0, VIEWPORT);
        speed.tick(1000.0 + SCROLL_SETTLE_MS);
        speed.sample(1_000_000.0, 1000.0 + SCROLL_SETTLE_MS + 1.0, VIEWPORT);
        assert_eq!(speed.motion(), Motion::Jump { stream: false });
    }

    #[test]
    fn the_place_is_remembered_across_a_pause_so_a_small_move_after_one_is_no_jump() {
        let mut speed = ScrollSpeed::default();
        speed.sample(50_000.0, 1000.0, VIEWPORT);
        speed.tick(1000.0 + SCROLL_SETTLE_MS);
        speed.sample(50_100.0, 5000.0, VIEWPORT);
        assert_eq!(speed.motion(), scroll(Direction::Down, 0.0, 0.0));
    }

    #[test]
    fn the_peak_is_kept_while_a_flick_slows_and_starts_again_on_a_reversal() {
        let mut speed = ScrollSpeed::default();
        speed.sample(0.0, 1000.0, VIEWPORT);
        speed.sample(160.0, 1016.0, VIEWPORT); // 10px/ms
        speed.sample(200.0, 1032.0, VIEWPORT); // 2.5px/ms
        assert_eq!(speed.motion(), scroll(Direction::Down, 2.5, 10.0));

        speed.sample(168.0, 1048.0, VIEWPORT); // back up at 2px/ms
        assert_eq!(speed.motion(), scroll(Direction::Up, 2.0, 2.0));
    }

    #[test]
    fn it_settles_once_the_samples_stop_and_says_when() {
        let mut speed = ScrollSpeed::default();
        assert_eq!(speed.settles_at(), None);
        speed.sample(0.0, 0.0, VIEWPORT);
        speed.sample(200.0, 16.0, VIEWPORT);
        assert_eq!(speed.settles_at(), Some(16.0 + SCROLL_SETTLE_MS));

        speed.tick(16.0 + SCROLL_SETTLE_MS - 1.0);
        assert!(matches!(speed.motion(), Motion::Scroll { .. }));
        speed.tick(16.0 + SCROLL_SETTLE_MS);
        assert_eq!(speed.motion(), Motion::Still);
        assert_eq!(speed.settles_at(), None);
    }

    #[test]
    fn the_measured_speed_stands_across_two_samples_with_the_same_time() {
        let mut speed = ScrollSpeed::default();
        speed.sample(0.0, 0.0, VIEWPORT);
        speed.sample(32.0, 16.0, VIEWPORT);
        speed.sample(500.0, 16.0, VIEWPORT);
        assert_eq!(speed.motion(), scroll(Direction::Down, 2.0, 2.0));
    }
}
