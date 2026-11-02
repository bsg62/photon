//! A player kept on a thread of its own, which the window only asks things of and reads
//! the news of. The system's player is opened, played, sought and let go of on that
//! thread, so that a video on a share that has stopped answering holds it and not the
//! window: GStreamer's `Null` joins its streaming threads and a flushing seek waits for
//! the read under way, Media Foundation's `Shutdown` waits, and each was a window that
//! stood still until the share answered. A thread held for good by such a share is left
//! behind, as the poster worker leaves a decoder.
//!
//! Not on a Mac: AVFoundation's player item is made on the main thread, which is the
//! window's (`System::open`).
//!
//! **What the window reads is the player as it was last read**, and a request it has made
//! is not in that yet: so a seek on its way is answered for here - the place it goes to,
//! and no end - or a video looped at its end would still be at its end, and loop again. A
//! frame from before the last seek is handed over while that seek is on its way - a drag
//! along the bar asks a seek a frame, and its frames are all from before the next - and
//! not once it is done, when the frame due is the seek's own: a paused seek's frame would
//! be a frame of the place it left. Whether the player has caught up is `waiting`, which
//! is how the window tells a seek's frame from one on the way to it.

use super::{Failure, Picture, Stream};
use std::{
    cell::Cell,
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

/// The players whose threads have not finished: the window waits for them as it closes,
/// for a while, so that no player is still being let go of as the process ends.
#[derive(Default)]
pub struct Live {
    count: Mutex<usize>,
    done: Condvar,
}

impl Live {
    /// Whether every player was let go of within `within`.
    pub fn finish(&self, within: Duration) -> bool {
        let until = Instant::now() + within;
        let mut count = lock(&self.count);
        while *count > 0 {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            count = (self.done.wait_timeout(count, left))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }
}

/// A player's thread, counted in `Live` until it ends - however it ends.
struct Alive(Arc<Live>);

impl Alive {
    fn new(live: &Arc<Live>) -> Self {
        *lock(&live.count) += 1;
        Alive(live.clone())
    }
}

impl Drop for Alive {
    fn drop(&mut self) {
        *lock(&self.0.count) -= 1;
        self.0.done.notify_all();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the window asks of the player, done in the order asked.
enum Ask {
    Play,
    Pause,
    Seek(f64),
    Volume(f64),
    Muted(bool),
}

/// The player as it was last read, on its thread.
#[derive(Default)]
struct News {
    /// Whether opening it is over, opened or not.
    opened: bool,
    /// How many requests had been done when it was read.
    done: u64,
    ready: bool,
    position: Option<f64>,
    duration: Option<f64>,
    ended: bool,
    failure: Option<Failure>,
    /// The newest frame the window has not taken, and how many requests had been done when
    /// the player handed it over.
    frame: Option<(u64, Picture)>,
}

#[derive(Default)]
struct Inner {
    asks: VecDeque<Ask>,
    /// The window has read the news, and fresher news is wanted.
    wanted: bool,
    /// The window has let go of the player.
    gone: bool,
    /// The thread is at the player, outside the lock.
    busy: bool,
    news: News,
}

#[derive(Default)]
struct Shared {
    inner: Mutex<Inner>,
    wake: Condvar,
}

/// A player on a thread of its own, as the window has it.
pub struct Detached {
    shared: Arc<Shared>,
    /// How many requests the window has made.
    sent: Cell<u64>,
    /// The last seek asked for: its number among the requests, and where it goes.
    sought: Cell<Option<(u64, f64)>>,
    /// Since when the player has been working at the oldest thing it has not answered.
    since: Cell<Instant>,
}

impl Detached {
    /// The player `open` makes, opened on a thread of its own, counted in `live`: this
    /// returns at once, and a player that cannot be opened says why by `failure`.
    pub fn open(
        live: &Arc<Live>,
        open: impl FnOnce() -> Result<Box<dyn Stream>, Failure> + Send + 'static,
    ) -> Self {
        let shared = Arc::new(Shared::default());
        let theirs = shared.clone();
        let alive = Alive::new(live);
        let spawned = std::thread::Builder::new()
            .name("video player".into())
            .spawn(move || {
                let _alive = alive;
                run(&theirs, open);
            });
        // The closure, `alive` with it, is dropped with the error.
        if let Err(err) = spawned {
            let mut inner = lock(&shared.inner);
            inner.news.failure = Some(Failure::Broken(format!("no thread for the player: {err}")));
            inner.news.opened = true;
        }
        Detached {
            shared,
            sent: Cell::new(0),
            sought: Cell::new(None),
            since: Cell::new(Instant::now()),
        }
    }

    /// The request's number. **A seek replaces one still queued** - a drag along the bar
    /// asks one a frame, and on a share that has stopped answering they would all be made
    /// once it answers - and takes its number.
    fn ask(&self, ask: Ask) -> u64 {
        let mut inner = lock(&self.shared.inner);
        if let (Ask::Seek(at), Some(Ask::Seek(queued))) = (&ask, inner.asks.back_mut()) {
            *queued = *at;
            return self.sent.get();
        }
        if !self.behind(&inner.news) {
            self.since.set(Instant::now());
        }
        let number = self.sent.get() + 1;
        self.sent.set(number);
        inner.asks.push_back(ask);
        drop(inner);
        self.shared.wake.notify_one();
        number
    }

    /// Whether the player has not answered everything asked of it.
    fn behind(&self, news: &News) -> bool {
        !news.opened || news.done < self.sent.get()
    }

    /// What the news says, and fresher news asked for: the player is read when the window
    /// reads it, and not while nothing does - a still window keeps a paused player's
    /// thread asleep.
    fn read<T>(&self, read: impl FnOnce(&mut News) -> T) -> T {
        let mut inner = lock(&self.shared.inner);
        inner.wanted = true;
        let read = read(&mut inner.news);
        drop(inner);
        self.shared.wake.notify_one();
        read
    }

    /// Where the last seek goes, while news from after it has not come.
    fn seeking(&self, done: u64) -> Option<f64> {
        (self.sought.get())
            .filter(|&(number, _)| done < number)
            .map(|(_, at)| at)
    }

    /// Until the thread has nothing to do: the player is opened, and what it was asked is
    /// done and read.
    #[cfg(test)]
    fn quiet(&self) {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            {
                let inner = lock(&self.shared.inner);
                if inner.news.opened && !inner.busy && !inner.wanted && inner.asks.is_empty() {
                    return;
                }
            }
            assert!(Instant::now() < until, "the player's thread never rested");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Drop for Detached {
    fn drop(&mut self) {
        let mut inner = lock(&self.shared.inner);
        inner.gone = true;
        inner.asks.clear();
        drop(inner);
        self.shared.wake.notify_one();
    }
}

impl Stream for Detached {
    fn ready(&self) -> bool {
        self.read(|news| news.ready)
    }
    fn play(&self) {
        self.ask(Ask::Play);
    }
    fn pause(&self) {
        self.ask(Ask::Pause);
    }
    fn seek(&self, at: f64) {
        let number = self.ask(Ask::Seek(at));
        self.sought.set(Some((number, at)));
    }
    fn set_volume(&self, volume: f64) {
        self.ask(Ask::Volume(volume));
    }
    fn set_muted(&self, muted: bool) {
        self.ask(Ask::Muted(muted));
    }
    fn position(&self) -> Option<f64> {
        self.read(|news| self.seeking(news.done).or(news.position))
    }
    fn duration(&self) -> Option<f64> {
        self.read(|news| news.duration)
    }
    fn ended(&self) -> bool {
        self.read(|news| news.ended && self.seeking(news.done).is_none())
    }
    fn failure(&self) -> Option<Failure> {
        self.read(|news| news.failure.clone())
    }
    fn frame(&self) -> Option<Picture> {
        let (taken, done) = self.read(|news| (news.frame.take(), news.done));
        let (read, frame) = taken?;
        (self.seeking(read).is_none() || self.seeking(done).is_some()).then_some(frame)
    }
    fn waiting(&self) -> Option<Duration> {
        self.read(|news| self.behind(news))
            .then(|| self.since.get().elapsed())
    }
}

/// The player's thread: opens it, does what it is asked and reads it while the window
/// wants news, and lets go of it once the window has - here, however long that takes.
fn run(shared: &Shared, open: impl FnOnce() -> Result<Box<dyn Stream>, Failure>) {
    let panicked = || Failure::Broken("the system's player failed".into());
    let stream = match catch_unwind(AssertUnwindSafe(open)).unwrap_or_else(|_| Err(panicked())) {
        Ok(stream) => stream,
        Err(failure) => {
            let mut inner = lock(&shared.inner);
            inner.news.failure = Some(failure);
            inner.news.opened = true;
            return;
        }
    };
    lock(&shared.inner).news.opened = true;
    // A player that panicked is a failure the window shows, not a thread that is gone and
    // a video that never begins.
    let served = catch_unwind(AssertUnwindSafe(|| serve(shared, stream.as_ref())));
    if served.is_err() {
        let mut inner = lock(&shared.inner);
        inner.news.failure = Some(panicked());
        inner.busy = false;
    }
    drop(stream);
}

fn serve(shared: &Shared, stream: &dyn Stream) {
    let mut done = 0u64;
    loop {
        let asks = {
            let mut inner = lock(&shared.inner);
            inner.busy = false;
            while !inner.gone && inner.asks.is_empty() && !inner.wanted {
                inner = shared
                    .wake
                    .wait(inner)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            if inner.gone {
                return;
            }
            inner.wanted = false;
            inner.busy = true;
            std::mem::take(&mut inner.asks)
        };
        // At the player, outside the lock: the window never waits for it.
        for ask in asks {
            match ask {
                Ask::Play => stream.play(),
                Ask::Pause => stream.pause(),
                Ask::Seek(at) => stream.seek(at),
                Ask::Volume(volume) => stream.set_volume(volume),
                Ask::Muted(muted) => stream.set_muted(muted),
            }
            done += 1;
        }
        let failure = stream.failure();
        let frame = stream.frame();
        let news = News {
            opened: true,
            done,
            ready: stream.ready(),
            position: stream.position(),
            duration: stream.duration(),
            ended: stream.ended(),
            failure,
            frame: None,
        };
        let mut inner = lock(&shared.inner);
        // A frame not yet taken is kept until a newer one comes.
        let kept = inner.news.frame.take();
        inner.news = News {
            frame: frame.map(|frame| (done, frame)).or(kept),
            ..news
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// A gate a stand-in player waits at, until it is opened.
    #[derive(Default)]
    struct Gate {
        open: Mutex<bool>,
        opened: Condvar,
    }

    impl Gate {
        fn closed() -> Arc<Gate> {
            Arc::new(Gate::default())
        }
        fn pass(&self) {
            let mut open = lock(&self.open);
            while !*open {
                open = self.opened.wait(open).unwrap();
            }
        }
        fn open(&self) {
            *lock(&self.open) = true;
            self.opened.notify_all();
        }
    }

    /// What the stand-in player was told, and what it has to say.
    #[derive(Default)]
    struct Told {
        log: Vec<String>,
        frames: VecDeque<Picture>,
        ended: bool,
        position: Option<f64>,
        dropped: bool,
    }

    /// A player that waits at its gate wherever `waits` names the call.
    struct Fake {
        told: Arc<Mutex<Told>>,
        gate: Arc<Gate>,
        waits: &'static [&'static str],
    }

    impl Fake {
        fn at(&self, call: &str) {
            if self.waits.contains(&call) {
                self.gate.pass();
            }
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.at("drop");
            lock(&self.told).dropped = true;
        }
    }

    impl Stream for Fake {
        fn ready(&self) -> bool {
            true
        }
        fn play(&self) {
            lock(&self.told).log.push("play".into());
        }
        fn pause(&self) {
            lock(&self.told).log.push("pause".into());
        }
        fn seek(&self, at: f64) {
            self.at("seek");
            let mut told = lock(&self.told);
            told.log.push(format!("seek {at}"));
            told.ended = false;
            told.position = Some(at);
        }
        fn set_volume(&self, volume: f64) {
            lock(&self.told).log.push(format!("volume {volume}"));
        }
        fn set_muted(&self, muted: bool) {
            lock(&self.told).log.push(format!("muted {muted}"));
        }
        fn position(&self) -> Option<f64> {
            lock(&self.told).position
        }
        fn duration(&self) -> Option<f64> {
            Some(4.0)
        }
        fn ended(&self) -> bool {
            lock(&self.told).ended
        }
        fn failure(&self) -> Option<Failure> {
            None
        }
        fn frame(&self) -> Option<Picture> {
            self.at("frame");
            lock(&self.told).frames.pop_front()
        }
    }

    fn picture(at: f64) -> Picture {
        Picture {
            width: 1,
            height: 1,
            at,
            rgba: vec![0, 0, 0, 255],
        }
    }

    struct Rig {
        told: Arc<Mutex<Told>>,
        gate: Arc<Gate>,
        live: Arc<Live>,
    }

    impl Rig {
        fn new() -> Rig {
            Rig {
                told: Default::default(),
                gate: Gate::closed(),
                live: Default::default(),
            }
        }

        /// A detached stand-in that waits at its gate where `waits` says, opening too.
        fn open(&self, waits: &'static [&'static str]) -> Detached {
            let fake = Fake {
                told: self.told.clone(),
                gate: self.gate.clone(),
                waits,
            };
            Detached::open(&self.live, move || {
                fake.at("open");
                Ok(Box::new(fake) as Box<dyn Stream>)
            })
        }
    }

    /// `f`, which must be over within a second: what the window does never waits for the
    /// player. Run on a thread of its own, so that a call that hangs fails the test rather
    /// than hanging it.
    fn at_once<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(Duration::from_secs(1))
            .expect("the window waited for the player")
    }

    /// Until `ok`, within ten seconds.
    fn until(what: &str, ok: impl Fn() -> bool) {
        let until = Instant::now() + Duration::from_secs(10);
        while !ok() {
            assert!(Instant::now() < until, "never {what}");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    // `Detached` is not `Send` - the window keeps it - so the tests that hang a call run
    // the whole of it, opening to letting go, on a thread of their own.

    #[test]
    fn a_player_whose_opening_hangs_holds_nothing_up() {
        let rig = Rig::new();
        let (told, gate, live) = (rig.told.clone(), rig.gate.clone(), rig.live.clone());
        at_once(move || {
            let player = rig.open(&["open"]);
            player.set_muted(true);
            player.play();
            assert!(!player.ready());
            assert!(player.waiting().is_some());
            assert_eq!(player.frame(), None);
            assert_eq!(player.failure(), None);
            drop(player);
        });
        assert!(
            !live.finish(Duration::ZERO),
            "a player held is not let go of"
        );
        // Once it opens, it is let go of, and told nothing the window asked of it after
        // it was gone.
        gate.open();
        assert!(live.finish(Duration::from_secs(10)));
        assert!(lock(&told).dropped);
        assert!(lock(&told).log.is_empty(), "{:?}", lock(&told).log);
    }

    #[test]
    fn a_player_whose_letting_go_hangs_holds_nothing_up() {
        let rig = Rig::new();
        let (told, gate, live) = (rig.told.clone(), rig.gate.clone(), rig.live.clone());
        at_once(move || {
            let player = rig.open(&["drop"]);
            until("ready", || player.ready());
            drop(player);
        });
        assert!(!live.finish(Duration::from_millis(50)));
        gate.open();
        assert!(live.finish(Duration::from_secs(10)));
        assert!(lock(&told).dropped);
    }

    #[test]
    fn a_seek_that_hangs_holds_nothing_up_and_is_answered_for_meanwhile() {
        let rig = Rig::new();
        lock(&rig.told).ended = true;
        lock(&rig.told).position = Some(4.0);
        let gate = rig.gate.clone();
        let live = rig.live.clone();
        at_once(move || {
            let player = rig.open(&["seek"]);
            until("at its end", || player.ended());
            // Looped at its end: the seek hangs on its thread, and is not at its end
            // meanwhile, but where it was sent.
            player.seek(0.0);
            player.play();
            assert!(!player.ended());
            assert_eq!(player.position(), Some(0.0));
            drop(player);
        });
        gate.open();
        assert!(live.finish(Duration::from_secs(10)));
    }

    #[test]
    fn requests_are_done_in_order_and_the_newest_frame_is_handed_over_once() {
        let rig = Rig::new();
        let player = rig.open(&[]);
        player.set_volume(0.5);
        player.set_muted(false);
        player.play();
        player.pause();
        player.quiet();
        assert_eq!(
            lock(&rig.told).log,
            ["volume 0.5", "muted false", "play", "pause"]
        );
        lock(&rig.told).frames.extend([picture(0.1), picture(0.2)]);
        // Read once to ask for it, and handed over once it has been read.
        let _ = player.frame();
        player.quiet();
        assert_eq!(player.frame().map(|frame| frame.at), Some(0.1));
        player.quiet();
        assert_eq!(player.frame().map(|frame| frame.at), Some(0.2));
        player.quiet();
        assert_eq!(player.frame(), None);
        assert_eq!(player.duration(), Some(4.0));
    }

    // A drag along the bar asks a seek a frame, and each frame read is from before the
    // next: shown all the same, while the player has not caught up.
    #[test]
    fn a_frame_from_before_a_seek_is_handed_over_while_the_seek_is_on_its_way() {
        let rig = Rig::new();
        let player = rig.open(&["seek"]);
        lock(&rig.told).frames.push_back(picture(3.0));
        let _ = player.frame();
        player.quiet();
        assert_eq!(player.waiting(), None);
        // The frame of 3 s is read and not yet taken, and a seek to 1 s is asked for,
        // which its thread is held in.
        player.seek(1.0);
        assert_eq!(player.frame().map(|frame| frame.at), Some(3.0));
        assert!(player.waiting().is_some(), "not the seek's own frame");
        rig.gate.open();
        player.quiet();
        assert_eq!(player.waiting(), None);
    }

    // Once the seek is done, the frame due is the seek's own: one read before it is not
    // handed over, or a paused seek would stand on the place it left.
    #[test]
    fn a_frame_from_before_a_seek_is_not_handed_over_once_it_is_done() {
        let rig = Rig::new();
        let player = rig.open(&[]);
        lock(&rig.told).frames.push_back(picture(3.0));
        let _ = player.frame();
        player.quiet();
        player.seek(1.0);
        player.quiet();
        assert_eq!(player.frame(), None);
        player.quiet();
        lock(&rig.told).frames.push_back(picture(1.0));
        let _ = player.frame();
        player.quiet();
        assert_eq!(player.frame().map(|frame| frame.at), Some(1.0));
    }

    #[test]
    fn a_seek_replaces_one_still_queued() {
        let rig = Rig::new();
        let player = rig.open(&["seek"]);
        player.quiet();
        player.seek(1.0);
        until("the first seek taken", || {
            lock(&player.shared.inner).asks.is_empty()
        });
        // Held in the first: the second waits in the queue, and the third takes its place.
        player.seek(2.0);
        player.seek(3.0);
        assert_eq!(player.position(), Some(3.0));
        rig.gate.open();
        player.quiet();
        assert_eq!(lock(&rig.told).log, ["seek 1", "seek 3"]);
        assert_eq!(player.position(), Some(3.0));
        assert_eq!(player.waiting(), None);
    }

    #[test]
    fn a_player_that_cannot_open_or_that_panics_says_so() {
        let live = Arc::new(Live::default());
        let refused = Detached::open(&live, || Err(Failure::Unsupported("no decoder".into())));
        until("its failure", || refused.failure().is_some());
        assert_eq!(
            refused.failure(),
            Some(Failure::Unsupported("no decoder".into()))
        );
        let panicked = Detached::open(&live, || panic!("the decoder fell over"));
        until("its failure", || panicked.failure().is_some());

        let rig = Rig::new();
        struct Bad(#[allow(dead_code)] Fake);
        impl Stream for Bad {
            fn ready(&self) -> bool {
                true
            }
            fn play(&self) {}
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
                None
            }
            fn frame(&self) -> Option<Picture> {
                panic!("a frame the decoder fell over on")
            }
        }
        let fake = Fake {
            told: rig.told.clone(),
            gate: rig.gate.clone(),
            waits: &[],
        };
        let bad = Detached::open(
            &rig.live,
            move || Ok(Box::new(Bad(fake)) as Box<dyn Stream>),
        );
        until("its failure", || bad.failure().is_some());
        // Let go of all the same.
        assert!(rig.live.finish(Duration::from_secs(10)));
        assert!(lock(&rig.told).dropped);
    }
}
