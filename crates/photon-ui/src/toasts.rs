//! Messages that go away by themselves: a step the engine refused, and later what an
//! action did. The Svelte UI's `library.toasts`.
//!
//! No egui here: time is a number, and `shell.rs` draws the list.

/// How long a failure stays. It is read, so it stays longer than a report.
pub const ERROR_MS: f64 = 6000.0;
/// How long a report of something that worked stays: read at a glance or not at all.
pub const DONE_MS: f64 = 4000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Error,
    Done,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub id: u64,
    pub message: String,
    pub kind: Kind,
    until_ms: f64,
}

#[derive(Default)]
pub struct Toasts {
    list: Vec<Toast>,
    next: u64,
}

impl Toasts {
    /// Says that something failed.
    pub fn error(&mut self, message: impl Into<String>, now_ms: f64) {
        self.push(message.into(), Kind::Error, now_ms + ERROR_MS);
    }

    /// Says what an action did.
    pub fn done(&mut self, message: impl Into<String>, now_ms: f64) {
        self.push(message.into(), Kind::Done, now_ms + DONE_MS);
    }

    fn push(&mut self, message: String, kind: Kind, until_ms: f64) {
        self.next += 1;
        self.list.push(Toast {
            id: self.next,
            message,
            kind,
            until_ms,
        });
    }

    pub fn dismiss(&mut self, id: u64) {
        self.list.retain(|toast| toast.id != id);
    }

    /// The messages to draw in a frame at `now_ms`, oldest first, and when the next of
    /// them ends: a still window draws no frame by itself, so the frame in which a
    /// message is to be gone has to be asked for.
    ///
    /// One call, so that the list a frame draws is never one from before its own time was
    /// taken off it: drawn first and let go of after, the frame asked for at a message's
    /// end drew the message once more, and nothing asked for another.
    pub fn at(&mut self, now_ms: f64) -> (&[Toast], Option<f64>) {
        self.list.retain(|toast| toast.until_ms > now_ms);
        let next = self
            .list
            .iter()
            .map(|toast| toast.until_ms)
            .min_by(f64::total_cmp);
        (&self.list, next)
    }

    /// The messages held, oldest first, whether or not their time is over.
    pub fn held(&self) -> &[Toast] {
        &self.list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(list: &[Toast]) -> Vec<&str> {
        list.iter().map(|toast| toast.message.as_str()).collect()
    }

    #[test]
    fn a_message_goes_away_by_itself_and_a_failure_stays_longer() {
        let mut toasts = Toasts::default();
        toasts.done("3 photos starred", 1_000.0);
        toasts.error("could not read the library", 1_000.0);
        // The frame to be drawn next is the one in which the first is over.
        let (showing, next) = toasts.at(1_000.0);
        assert_eq!(showing.len(), 2);
        assert_eq!(next, Some(5_000.0));
        assert_eq!(toasts.at(4_999.0).1, Some(5_000.0));

        let (showing, next) = toasts.at(5_000.0);
        assert_eq!(said(showing), ["could not read the library"]);
        assert_eq!(showing[0].kind, Kind::Error);
        assert_eq!(next, Some(7_000.0));

        let (showing, next) = toasts.at(7_000.0);
        assert!(showing.is_empty());
        assert_eq!(next, None);
    }

    // The frame a message's end asked for is the frame it is gone in. Handed the list as
    // it was before that frame's time was taken off it, the frame drew the message once
    // more, and on a still window that picture stayed.
    #[test]
    fn the_frame_asked_for_at_a_messages_end_does_not_draw_it() {
        let mut toasts = Toasts::default();
        toasts.error("refused", 0.0);
        let (_, next) = toasts.at(16.0);
        let end = next.expect("a frame is asked for at its end");
        let (showing, next) = toasts.at(end);
        assert!(showing.is_empty(), "{:?}", said(showing));
        assert_eq!(next, None);
    }

    #[test]
    fn a_message_dismissed_goes_at_once_and_takes_no_other_with_it() {
        let mut toasts = Toasts::default();
        toasts.error("one", 0.0);
        toasts.error("two", 0.0);
        toasts.error("three", 0.0);
        // The one in the middle: neither the older nor the newer goes with it.
        let second = toasts.held()[1].id;
        toasts.dismiss(second);
        assert_eq!(said(toasts.held()), ["one", "three"]);
        // An id is never given twice, so a late dismissal of one long gone hits nothing.
        toasts.error("four", 0.0);
        toasts.dismiss(second);
        assert_eq!(said(toasts.held()), ["one", "three", "four"]);
    }
}
