//! Thumbnails on their way from the cache to the UI thread: read and decoded on a small
//! pool, or waited for when they have not been built yet.
//!
//! **A thumbnail not built yet is waited for, never blocked on.** Every such wait is a
//! future, and one thread polls them all. `protocol.rs` records why: when each waiting
//! request held a thread for up to `THUMB_TIMEOUT`, a fast scroll through a fresh import
//! parked hundreds of them, and thumbnails that *were* cached queued behind. A decoder here
//! never waits: a miss is handed to the waiter and the decoder takes the next thumbnail.

use super::textures::Pixels;
use parking_lot::{Condvar, Mutex};
use std::{
    collections::HashSet,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    task::{Context, Poll, Wake, Waker},
    thread::{self, Thread},
    time::{Duration, Instant},
};

/// A photo whose thumbnail the grid wants: the photo, to have it built, and the key its
/// picture is cached under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Want {
    pub id: i64,
    pub key: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// It cannot be made: the file is unreadable or not a picture. Asking again will not help.
    Failed(String),
    /// It was not built in time, or the photo is gone. Worth asking again when it is next
    /// wanted.
    Unavailable,
}

#[derive(Debug)]
pub struct Loaded {
    pub key: u64,
    pub result: Result<Pixels, LoadError>,
}

pub type Building<'a> = Pin<Box<dyn Future<Output = Result<Pixels, LoadError>> + Send + 'a>>;

/// Where thumbnails come from. The engine in the application; a fake in the tests, which
/// is the reason this is a trait.
pub trait ThumbSource: Send + Sync + 'static {
    /// The thumbnail cached under `key`, decoded; `None` when none is cached. Never waits
    /// for one to be built.
    fn cached(&self, key: u64) -> Option<Pixels>;
    /// The thumbnail `want` names, once it has been built: the picture cached under
    /// `want.key`, and an error if the photo has another by then. Dropping the future gives
    /// up the wait.
    fn build(&self, want: Want) -> Building<'_>;
}

struct State {
    /// What the grid wants now, most wanted first. A decoder takes the first one nobody
    /// is working on.
    wanted: Vec<Want>,
    /// The keys of the latest `want`, kept after a decoder has taken its entry from
    /// `wanted`: what the waiter asks to know whether a wait is still worth holding.
    keys: HashSet<u64>,
    /// Keys being decoded or waited for.
    busy: HashSet<u64>,
    /// Keys whose result is in the channel and has not been taken by `poll`. The grid
    /// still lists them as wanted on every frame until then, and a decoder that took one
    /// again would read the same file a second time.
    handed_back: HashSet<u64>,
    closed: bool,
}

struct Shared {
    state: Mutex<State>,
    work: Condvar,
    notify: Box<dyn Fn() + Send + Sync>,
}

impl Shared {
    fn finish(&self, loaded: &Sender<Loaded>, key: u64, result: Result<Pixels, LoadError>) {
        {
            let mut state = self.state.lock();
            state.busy.remove(&key);
            state.handed_back.insert(key);
        }
        if loaded.send(Loaded { key, result }).is_ok() {
            (self.notify)();
        }
    }
}

pub struct Loader {
    shared: Arc<Shared>,
    loaded: Receiver<Loaded>,
    waiter: Thread,
}

struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

impl Loader {
    /// Starts `decoders` decoding threads and the one waiting thread. `timeout` bounds a
    /// wait for a thumbnail to be built; `notify` is called from a worker thread after each
    /// result is ready.
    pub fn spawn<S: ThumbSource>(
        source: Arc<S>,
        decoders: usize,
        timeout: Duration,
        notify: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                wanted: Vec::new(),
                keys: HashSet::new(),
                busy: HashSet::new(),
                handed_back: HashSet::new(),
                closed: false,
            }),
            work: Condvar::new(),
            notify: Box::new(notify),
        });
        let (loaded_tx, loaded) = mpsc::channel();
        let (misses_tx, misses) = mpsc::channel::<Want>();

        let waiter = {
            let (shared, source, loaded_tx) = (shared.clone(), source.clone(), loaded_tx.clone());
            thread::Builder::new()
                .name("thumb-wait".to_owned())
                .spawn(move || wait_for_builds(&shared, &*source, &misses, &loaded_tx, timeout))
                .expect("the thumbnail waiter could not be started")
                .thread()
                .clone()
        };
        for n in 0..decoders.max(1) {
            let (shared, source, loaded_tx, misses_tx, waiter) = (
                shared.clone(),
                source.clone(),
                loaded_tx.clone(),
                misses_tx.clone(),
                waiter.clone(),
            );
            thread::Builder::new()
                .name(format!("thumb-decode-{n}"))
                .spawn(move || decode(&shared, &*source, &loaded_tx, &misses_tx, &waiter))
                .expect("a thumbnail decoder could not be started");
        }
        Self {
            shared,
            loaded,
            waiter,
        }
    }

    /// What the grid wants now, most wanted first, replacing what it wanted before. A
    /// thumbnail being waited for that is not in the list is given up.
    pub fn want(&self, wanted: Vec<Want>) {
        {
            let mut state = self.shared.state.lock();
            state.keys = wanted.iter().map(|want| want.key).collect();
            state.wanted = wanted;
        }
        self.shared.work.notify_all();
        self.waiter.unpark();
    }

    /// Every result ready now. From here on its key may be read again, should the grid
    /// still want it after what it does with the result.
    pub fn poll(&self) -> Vec<Loaded> {
        let loaded: Vec<Loaded> = self.loaded.try_iter().collect();
        if !loaded.is_empty() {
            let mut state = self.shared.state.lock();
            for result in &loaded {
                state.handed_back.remove(&result.key);
            }
        }
        loaded
    }
}

/// The threads end at their next wait. They are not joined: a decode in flight is a tenth
/// of a millisecond, and a wait is given up by being dropped.
impl Drop for Loader {
    fn drop(&mut self) {
        self.shared.state.lock().closed = true;
        self.shared.work.notify_all();
        self.waiter.unpark();
    }
}

fn decode<S: ThumbSource>(
    shared: &Shared,
    source: &S,
    loaded: &Sender<Loaded>,
    misses: &Sender<Want>,
    waiter: &Thread,
) {
    loop {
        let want = {
            let mut state = shared.state.lock();
            loop {
                if state.closed {
                    return;
                }
                let free = state.wanted.iter().position(|want| {
                    !state.busy.contains(&want.key) && !state.handed_back.contains(&want.key)
                });
                if let Some(free) = free {
                    let want = state.wanted.remove(free);
                    state.busy.insert(want.key);
                    break want;
                }
                shared.work.wait(&mut state);
            }
        };
        match source.cached(want.key) {
            Some(pixels) => shared.finish(loaded, want.key, Ok(pixels)),
            // Not built yet. The key stays busy; the waiter finishes it.
            None => {
                if misses.send(want).is_err() {
                    return;
                }
                waiter.unpark();
            }
        }
    }
}

fn wait_for_builds<S: ThumbSource>(
    shared: &Shared,
    source: &S,
    misses: &Receiver<Want>,
    loaded: &Sender<Loaded>,
    timeout: Duration,
) {
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut waiting: Vec<(Want, Building<'_>, Instant)> = Vec::new();
    loop {
        if shared.state.lock().closed {
            return;
        }
        for want in misses.try_iter() {
            waiting.push((want, source.build(want), Instant::now() + timeout));
        }
        let now = Instant::now();
        let mut next = 0;
        while next < waiting.len() {
            let (want, building, deadline) = &mut waiting[next];
            let key = want.key;
            // `None`: given up, nothing to say. Dropping the future is what gives up.
            let outcome = if !shared.state.lock().keys.contains(&key) {
                Some(None)
            } else if let Poll::Ready(result) = building.as_mut().poll(&mut context) {
                Some(Some(result))
            } else if now >= *deadline {
                Some(Some(Err(LoadError::Unavailable)))
            } else {
                None
            };
            match outcome {
                None => next += 1,
                Some(result) => {
                    drop(waiting.swap_remove(next));
                    match result {
                        Some(result) => shared.finish(loaded, key, result),
                        None => {
                            shared.state.lock().busy.remove(&key);
                        }
                    }
                }
            }
        }
        // Woken by a miss, a new `want`, a build finishing or `Drop`; otherwise by the
        // nearest deadline.
        match waiting.iter().map(|(_, _, deadline)| *deadline).min() {
            Some(deadline) => thread::park_timeout(deadline.saturating_duration_since(now)),
            None => thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn pixels(key: u64) -> Pixels {
        Pixels {
            width: 1,
            height: 1,
            rgba: vec![key as u8; 4],
        }
    }

    /// A source whose cache and whose builds the test controls. A photo's key is its id.
    #[derive(Default)]
    struct Fake {
        cached: Mutex<HashSet<u64>>,
        /// How often the cache was read.
        reads: AtomicUsize,
        built: Mutex<HashSet<i64>>,
        wakers: Mutex<Vec<Waker>>,
        given_up: AtomicUsize,
    }

    impl Fake {
        fn finish_building(&self, id: i64) {
            self.built.lock().insert(id);
            for waker in self.wakers.lock().drain(..) {
                waker.wake();
            }
        }
    }

    struct FakeBuild<'a> {
        fake: &'a Fake,
        id: i64,
        done: bool,
    }

    impl Future for FakeBuild<'_> {
        type Output = Result<Pixels, LoadError>;
        fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
            if self.fake.built.lock().contains(&self.id) {
                self.done = true;
                return Poll::Ready(Ok(pixels(self.id as u64)));
            }
            self.fake.wakers.lock().push(context.waker().clone());
            Poll::Pending
        }
    }

    impl Drop for FakeBuild<'_> {
        fn drop(&mut self) {
            if !self.done {
                self.fake.given_up.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    impl ThumbSource for Fake {
        fn cached(&self, key: u64) -> Option<Pixels> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.cached.lock().contains(&key).then(|| pixels(key))
        }
        fn build(&self, want: Want) -> Building<'_> {
            Box::pin(FakeBuild {
                fake: self,
                id: want.id,
                done: false,
            })
        }
    }

    fn want(id: i64) -> Want {
        Want { id, key: id as u64 }
    }

    fn loader(fake: &Arc<Fake>, decoders: usize, timeout: Duration) -> Loader {
        Loader::spawn(fake.clone(), decoders, timeout, || {})
    }

    const LONG: Duration = Duration::from_secs(600);

    /// Results until `count` have come, or panics after ten seconds.
    fn results(loader: &Loader, count: usize) -> Vec<Loaded> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut all = Vec::new();
        while all.len() < count {
            all.extend(loader.poll());
            assert!(
                Instant::now() < deadline,
                "{} of {count} results",
                all.len()
            );
            thread::sleep(Duration::from_millis(2));
        }
        all
    }

    /// Waits until `condition` holds, or panics after ten seconds.
    fn eventually(what: &str, condition: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !condition() {
            assert!(Instant::now() < deadline, "never: {what}");
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn a_cached_thumbnail_is_read_and_handed_back_under_its_key() {
        let fake = Arc::new(Fake::default());
        fake.cached.lock().insert(7);
        let loader = loader(&fake, 2, LONG);
        loader.want(vec![want(7)]);
        let got = results(&loader, 1);
        assert_eq!(got[0].key, 7);
        assert_eq!(got[0].result, Ok(pixels(7)));
    }

    #[test]
    fn a_thumbnail_not_built_yet_comes_once_it_is() {
        let fake = Arc::new(Fake::default());
        let loader = loader(&fake, 1, LONG);
        loader.want(vec![want(3)]);
        eventually("the build is being waited for", || {
            !fake.wakers.lock().is_empty()
        });
        assert!(loader.poll().is_empty());
        fake.finish_building(3);
        let got = results(&loader, 1);
        assert_eq!((got[0].key, &got[0].result), (3, &Ok(pixels(3))));
    }

    // The lesson of `protocol.rs`: with waits on the decoding threads, five unbuilt
    // thumbnails ahead of a cached one would hold the one decoder for ever.
    #[test]
    fn waits_for_unbuilt_thumbnails_do_not_hold_up_a_cached_one() {
        let fake = Arc::new(Fake::default());
        fake.cached.lock().insert(99);
        let loader = loader(&fake, 1, LONG);
        let mut wanted: Vec<Want> = (1..=5).map(want).collect();
        wanted.push(want(99));
        loader.want(wanted);
        let got = results(&loader, 1);
        assert_eq!(got[0].key, 99);
        assert_eq!(
            fake.given_up.load(Ordering::SeqCst),
            0,
            "the five are still waited for"
        );
    }

    #[test]
    fn a_wait_for_a_thumbnail_no_longer_wanted_is_given_up() {
        let fake = Arc::new(Fake::default());
        let loader = loader(&fake, 1, LONG);
        loader.want(vec![want(3)]);
        eventually("the build is being waited for", || {
            !fake.wakers.lock().is_empty()
        });
        loader.want(Vec::new());
        eventually("the wait is dropped", || {
            fake.given_up.load(Ordering::SeqCst) == 1
        });
        assert!(loader.poll().is_empty(), "and nothing is said about it");
        // Wanted again later, it is waited for again and delivered.
        loader.want(vec![want(3)]);
        fake.finish_building(3);
        assert_eq!(results(&loader, 1)[0].key, 3);
    }

    #[test]
    fn a_thumbnail_not_built_in_time_is_unavailable() {
        let fake = Arc::new(Fake::default());
        let loader = loader(&fake, 1, Duration::from_millis(30));
        loader.want(vec![want(3)]);
        let got = results(&loader, 1);
        assert_eq!(
            (got[0].key, &got[0].result),
            (3, &Err(LoadError::Unavailable))
        );
    }

    #[test]
    fn the_ui_is_told_when_a_result_is_ready() {
        let fake = Arc::new(Fake::default());
        fake.cached.lock().insert(1);
        let told = Arc::new(AtomicUsize::new(0));
        let counter = told.clone();
        let loader = Loader::spawn(fake, 1, LONG, move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        loader.want(vec![want(1)]);
        results(&loader, 1);
        eventually("told once", || told.load(Ordering::SeqCst) == 1);
    }

    // The grid lists what it wants every frame, and a thumbnail is wanted until the frame
    // that takes it from `poll`. Read again on each of those frames, a screenful on a cold
    // disk was read one and a half times over (measured in review: 96 reads for 60).
    #[test]
    fn a_thumbnail_handed_back_and_not_yet_taken_is_not_read_again() {
        let fake = Arc::new(Fake::default());
        fake.cached.lock().insert(1);
        let told = Arc::new(AtomicUsize::new(0));
        let counter = told.clone();
        let loader = Loader::spawn(fake.clone(), 1, LONG, move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        loader.want(vec![want(1)]);
        eventually("handed back", || told.load(Ordering::SeqCst) == 1);
        for _ in 0..20 {
            loader.want(vec![want(1)]);
            thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(fake.reads.load(Ordering::SeqCst), 1);
        assert_eq!(loader.poll().len(), 1);
        // Taken, and wanted again - its texture was let go: now it is read again.
        loader.want(vec![want(1)]);
        results(&loader, 1);
        assert_eq!(fake.reads.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn the_most_wanted_is_decoded_first() {
        let fake = Arc::new(Fake::default());
        fake.cached.lock().extend([1, 2, 3]);
        let loader = loader(&fake, 1, LONG);
        loader.want(vec![want(2), want(3), want(1)]);
        let order: Vec<u64> = results(&loader, 3).iter().map(|l| l.key).collect();
        assert_eq!(order, [2, 3, 1]);
    }
}
