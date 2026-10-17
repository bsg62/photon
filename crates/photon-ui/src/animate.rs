//! A value that slides to its target over time: the sidebar's width and the info panel's
//! room move by it. Time is milliseconds on the application's clock, as a number, so the
//! slide is measured in time and not in frames - a 120 Hz screen does not run it twice as
//! fast - and a test has no clock to wait for.

/// How long a slide takes, whatever its distance.
pub const DURATION_MS: f64 = 200.0;

/// Fast at the start and settling at the end, a cubic: 0.875 at the half.
pub fn ease_out(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slide {
    from: f64,
    to: f64,
    started_ms: f64,
}

impl Slide {
    /// Settled at `value`.
    pub fn at(value: f64) -> Self {
        Slide {
            from: value,
            to: value,
            started_ms: 0.0,
        }
    }

    /// Slides on from the value drawn at `now_ms`, so a target changed in the middle of a
    /// slide turns round without a jump. The target it already has changes nothing: asked
    /// again in every frame, it must not restart the slide. With `animate` off the value
    /// jumps, which also ends a slide that was running.
    pub fn to(&mut self, target: f64, now_ms: f64, animate: bool) {
        if !animate {
            *self = Slide::at(target);
            return;
        }
        if target == self.to {
            return;
        }
        *self = Slide {
            from: self.value(now_ms),
            to: target,
            started_ms: now_ms,
        };
    }

    pub fn value(&self, now_ms: f64) -> f64 {
        let t = (now_ms - self.started_ms) / DURATION_MS;
        self.from + (self.to - self.from) * ease_out(t)
    }

    pub fn target(&self) -> f64 {
        self.to
    }

    /// Whether the value is still on its way, for the frames a still window does not draw.
    pub fn running(&self, now_ms: f64) -> bool {
        self.from != self.to && now_ms - self.started_ms < DURATION_MS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slide_eases_out_over_its_time() {
        let mut slide = Slide::at(0.0);
        slide.to(1.0, 1000.0, true);
        assert_eq!(slide.value(1000.0), 0.0);
        assert_eq!(slide.value(1100.0), 0.875);
        assert_eq!(ease_out(0.5), 0.875);
        assert_eq!(slide.value(1200.0), 1.0);
        assert!(slide.running(1199.0));
        assert!(!slide.running(1200.0));
    }

    #[test]
    fn a_slide_turned_round_starts_from_where_it_was() {
        let mut slide = Slide::at(0.0);
        slide.to(1.0, 0.0, true);
        slide.to(0.0, 100.0, true);
        assert_eq!(slide.value(100.0), 0.875);
        assert_eq!(slide.value(300.0), 0.0);
    }

    #[test]
    fn asking_for_the_target_again_does_not_restart_it() {
        let mut slide = Slide::at(0.0);
        slide.to(1.0, 0.0, true);
        slide.to(1.0, 150.0, true);
        assert!(!slide.running(200.0));
    }

    #[test]
    fn off_ends_a_running_slide() {
        let mut slide = Slide::at(0.0);
        slide.to(1.0, 0.0, true);
        slide.to(1.0, 50.0, false);
        assert_eq!(slide.value(50.0), 1.0);
        assert!(!slide.running(50.0));
    }

    #[test]
    fn progress_is_time_not_frames() {
        let mut slide = Slide::at(0.0);
        slide.to(1.0, 0.0, true);
        // Frame by frame at 120 Hz and at 60 Hz, as a window would ask: the value at
        // 100 ms does not depend on how many frames it took to get there.
        let (mut fast, mut slow) = (0.0, 0.0);
        for n in 1..=12 {
            fast = slide.value(f64::from(n) * 100.0 / 12.0);
        }
        for n in 1..=6 {
            slow = slide.value(f64::from(n) * 100.0 / 6.0);
        }
        assert!((fast - slow).abs() < 1e-9);
        assert!((fast - 0.875).abs() < 1e-6);
    }
}
