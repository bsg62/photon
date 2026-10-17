//! The People page's crops, cut off the window's thread: a face's square from its photo's
//! cached 1600-pixel preview, `CROP_PX` on a side, as `/face/` in `protocol.rs` cuts them -
//! by the engine's own `ThumbCache::face_crop`, which keeps the last eight decoded previews
//! so that a photo of many faces is decoded once. Never from the photo, and never a render:
//! a face whose preview is not cached is `Missing`, and the page draws an empty square.
//!
//! At most `MAX_CUTTERS` cuts at a time, half the cores on a smaller machine: a crop decodes a
//! whole preview unless its photo's is kept, and a page asks for one per face in sight - one
//! cutter a core held a 1600-pixel decode on each of sixteen threads at once, memory the
//! allocator keeps after the page is gone. A crop is named by its face and its thumbnail key -
//! a key names one picture, so a crop made is never stale, and a face whose photo was edited
//! since is another crop. When the page closes the engine's kept previews are let go of
//! (`forget`), once now and once more after each cut that was on its way, which kept one.

use parking_lot::{Condvar, Mutex};
use photon_engine::engine::Engine;
use std::{
    collections::{HashSet, VecDeque},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread,
};

pub use photon_core::thumbs::face_crop::CROP_PX;

/// A face's crop: the face's id and its photo's thumbnail key, as the page lists them.
pub type CropOf = (i64, String);

/// The most cutters there are, whatever the cores.
const MAX_CUTTERS: usize = 4;

/// What came of cutting a crop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Made {
    /// `CROP_PX` square, RGBA.
    Pixels(Vec<u8>),
    /// Nothing to cut it from: the preview is not cached, the face is gone, or its photo's
    /// key has moved on since the page was read.
    Missing,
    /// The preview could not be read.
    Failed,
}

#[derive(Default)]
struct Line {
    waiting: VecDeque<(u64, CropOf)>,
    /// The opening of the page what is cut now is for: a cut of an older one is dropped,
    /// and what it kept forgotten.
    opening: u64,
    closed: bool,
}

struct Shared {
    line: Mutex<Line>,
    wake: Condvar,
}

/// The cutters, and what has been asked of them since the page was last opened.
pub struct Crops {
    shared: Arc<Shared>,
    forget: Arc<dyn Fn() + Send + Sync>,
    answers: Receiver<(u64, CropOf, Made)>,
    /// Moves when the page closes: an answer of an older opening is dropped.
    opening: u64,
    asked: HashSet<CropOf>,
}

impl Crops {
    /// The cutters, each cutting with `cut` and calling `notify` with every crop made;
    /// `forget` lets go of what cutting kept, once the page is closed.
    pub fn spawn(
        cut: impl Fn(&CropOf) -> Made + Send + Sync + 'static,
        forget: impl Fn() + Send + Sync + 'static,
        notify: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            line: Mutex::new(Line::default()),
            wake: Condvar::new(),
        });
        let (tx, answers) = mpsc::channel();
        let cut = Arc::new(cut);
        let forget: Arc<dyn Fn() + Send + Sync> = Arc::new(forget);
        let notify = Arc::new(notify);
        let cores = thread::available_parallelism().map_or(1, |n| n.get());
        for n in 0..(cores / 2).clamp(1, MAX_CUTTERS) {
            let (shared, tx, cut, forget, notify) = (
                shared.clone(),
                tx.clone(),
                cut.clone(),
                forget.clone(),
                notify.clone(),
            );
            thread::Builder::new()
                .name(format!("face crop {n}"))
                .spawn(move || cutter(&shared, &tx, &*cut, &*forget, &*notify))
                .expect("a thread for the face crops");
        }
        Self {
            shared,
            forget,
            answers,
            opening: 0,
            asked: HashSet::new(),
        }
    }

    /// Cut by the engine, from its cache.
    pub fn of(engine: Arc<Engine>, notify: impl Fn() + Send + Sync + 'static) -> Self {
        let kept = engine.clone();
        Self::spawn(
            move |of| cut(&engine, of),
            move || kept.thumbs.forget_decoded(),
            notify,
        )
    }

    /// Asks for the crop of `of`, once a page: one asked for already is on its way or
    /// made, and one that could not be made is not asked for again until the page opens
    /// again.
    pub fn ask(&mut self, of: &CropOf) {
        if !self.asked.insert(of.clone()) {
            return;
        }
        self.shared
            .line
            .lock()
            .waiting
            .push_back((self.opening, of.clone()));
        self.shared.wake.notify_one();
    }

    /// The crop of `of` has been let go of: it is asked for again when it is next in sight.
    pub fn forget(&mut self, of: &CropOf) {
        self.asked.remove(of);
    }

    /// The crops made since this was last asked, of this opening of the page.
    pub fn answers(&mut self) -> Vec<(CropOf, Made)> {
        let opening = self.opening;
        (self.answers.try_iter())
            .filter(|(from, _, _)| *from == opening)
            .map(|(_, of, made)| (of, made))
            .collect()
    }

    /// The page is closed: what waits is not cut, what is being cut is dropped as it
    /// arrives, and what cutting kept is let go of.
    pub fn clear(&mut self) {
        self.opening += 1;
        self.asked.clear();
        {
            let mut line = self.shared.line.lock();
            line.waiting.clear();
            line.opening = self.opening;
        }
        (self.forget)();
    }
}

impl Drop for Crops {
    fn drop(&mut self) {
        self.shared.line.lock().closed = true;
        self.shared.wake.notify_all();
    }
}

fn cutter(
    shared: &Shared,
    tx: &Sender<(u64, CropOf, Made)>,
    cut: &(dyn Fn(&CropOf) -> Made + Send + Sync),
    forget: &(dyn Fn() + Send + Sync),
    notify: &(dyn Fn() + Send + Sync),
) {
    loop {
        let (opening, of) = {
            let mut line = shared.line.lock();
            loop {
                if line.closed {
                    return;
                }
                if let Some(job) = line.waiting.pop_front() {
                    break job;
                }
                shared.wake.wait(&mut line);
            }
        };
        // A preview whose decode panics costs its crops and no more.
        let made = catch_unwind(AssertUnwindSafe(|| cut(&of))).unwrap_or(Made::Failed);
        // The page closed while this was cut: `clear` has forgotten what was kept, and this
        // cut has just kept its preview again.
        if shared.line.lock().opening != opening {
            forget();
        }
        if tx.send((opening, of, made)).is_err() {
            return;
        }
        notify();
    }
}

/// The engine's crop of a face, as the route cuts it, decoded: the page draws pixels.
fn cut(engine: &Engine, (face, key): &CropOf) -> Made {
    let Ok(key) = u64::from_str_radix(key, 16) else {
        return Made::Missing;
    };
    match engine.lib.face_crop_source(*face) {
        Ok(Some((rect, current))) if current == key => match engine.thumbs.face_crop(key, &rect) {
            Ok(bytes) => match webp::Decoder::new(&bytes).decode() {
                Some(image) => Made::Pixels(image.to_image().to_rgba8().into_raw()),
                None => Made::Failed,
            },
            Err(photon_core::Error::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
                Made::Missing
            }
            Err(_) => Made::Failed,
        },
        Ok(_) => Made::Missing,
        Err(_) => Made::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_for(crops: &mut Crops, n: usize) -> Vec<(CropOf, Made)> {
        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        while got.len() < n {
            assert!(Instant::now() < deadline, "only {got:?}");
            got.extend(crops.answers());
            thread::sleep(Duration::from_millis(1));
        }
        got
    }

    fn of(face: i64) -> CropOf {
        (face, format!("{face:016x}"))
    }

    // Each crop is asked for once a page, whatever is drawn how often; a page opened again
    // asks again.
    #[test]
    fn a_crop_is_asked_for_once_an_opening() {
        let cuts = Arc::new(Mutex::new(Vec::new()));
        let seen = cuts.clone();
        let mut crops = Crops::spawn(
            move |of| {
                seen.lock().push(of.0);
                Made::Missing
            },
            || {},
            || {},
        );
        crops.ask(&of(1));
        crops.ask(&of(1));
        crops.ask(&of(2));
        let got = wait_for(&mut crops, 2);
        assert_eq!(got.len(), 2);
        crops.ask(&of(1));
        thread::sleep(Duration::from_millis(20));
        assert!(crops.answers().is_empty());
        crops.clear();
        crops.ask(&of(1));
        wait_for(&mut crops, 1);
        let mut cut = cuts.lock().clone();
        cut.sort_unstable();
        assert_eq!(cut, [1, 1, 2]);
    }

    // A crop being cut when the page closes is dropped as it arrives: it is of another
    // opening.
    #[test]
    fn a_crop_of_a_page_closed_is_dropped() {
        let (go, wait) = mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let mut crops = Crops::spawn(
            move |_| {
                let _ = wait.lock().recv();
                Made::Missing
            },
            || {},
            || {},
        );
        crops.ask(&of(1));
        thread::sleep(Duration::from_millis(20));
        crops.clear();
        go.send(()).unwrap();
        thread::sleep(Duration::from_millis(50));
        assert!(crops.answers().is_empty());
    }

    // What cutting kept is let go of when the page closes, and again once a cut that was on
    // its way then is done - it has kept its preview after the first.
    #[test]
    fn a_page_closed_forgets_what_its_cuts_kept() {
        let (go, wait) = mpsc::channel::<()>();
        let wait = Mutex::new(wait);
        let forgotten = Arc::new(Mutex::new(0));
        let counted = forgotten.clone();
        let (done, finished) = mpsc::channel::<()>();
        let done = Mutex::new(done);
        let mut crops = Crops::spawn(
            move |_| {
                let _ = wait.lock().recv();
                Made::Missing
            },
            move || *counted.lock() += 1,
            move || {
                let _ = done.lock().send(());
            },
        );
        crops.ask(&of(1));
        thread::sleep(Duration::from_millis(20));
        assert_eq!(*forgotten.lock(), 0);
        crops.clear();
        assert_eq!(*forgotten.lock(), 1);
        go.send(()).unwrap();
        finished.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(*forgotten.lock(), 2);
        // A cut for the page that is open forgets nothing.
        crops.ask(&of(2));
        go.send(()).unwrap();
        finished.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(*forgotten.lock(), 2);
    }

    // A cut that panics is that crop failed, and the cutters go on.
    #[test]
    fn a_cut_that_panics_fails_its_crop_alone() {
        let mut crops = Crops::spawn(
            |of| {
                assert!(of.0 != 1, "a preview no decoder can read");
                Made::Missing
            },
            || {},
            || {},
        );
        crops.ask(&of(1));
        crops.ask(&of(2));
        let mut got = wait_for(&mut crops, 2);
        got.sort_by_key(|(of, _)| of.0);
        assert_eq!(got, [(of(1), Made::Failed), (of(2), Made::Missing)]);
    }
}
