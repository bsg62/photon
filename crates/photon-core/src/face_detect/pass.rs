//! One pass over the photos the detector has not looked at: read each one's cached
//! preview, detect, write. The engine owns the thread, the triggers and the progress; this
//! is the loop, here so it can be run against a library and a cache without an engine.

use super::Detection;
use crate::Result;
use crate::library::{FaceCandidate, Library};
use crate::thumbs::{ThumbCache, ThumbSize};
use image::DynamicImage;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Photos per write. One transaction a batch: small enough that a quit loses under a
/// minute of work, large enough that the writer is not taken per photo.
pub const BATCH: usize = 64;

/// What finds the faces in a preview: `Detector::detect`, or a test's stand-in.
pub type Detect<'a> = dyn Fn(&DynamicImage) -> Result<Vec<Detection>> + Sync + 'a;

/// Half the machine's cores, at least one and at most four. A worker holds 100-120 MB at
/// the detector's input size (measured: 159 MB with one, 501 MB with four), so the cap is
/// what bounds the pass's memory; the half is what leaves the machine usable for the
/// hours a first pass over a large library takes.
pub fn workers() -> usize {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    (cores / 2).clamp(1, 4)
}

/// Detects every candidate, in batches of [`BATCH`]. Returns how many photos were written;
/// `on_batch` is told each batch's count as it lands.
///
/// **Pages by id.** A photo whose preview cannot be read is skipped without being written,
/// so it is still a candidate - asked for again from the start, it would be handed back for
/// ever. Paging on from the last id read ends this pass; the next pass asks again, which
/// is when a preview that has come back (a cleared cache rendered anew) is detected.
pub fn run(
    lib: &Library,
    cache: &ThumbCache,
    version: i64,
    workers: usize,
    detect: &Detect<'_>,
    cancel: &(dyn Fn() -> bool + Sync),
    on_batch: &mut dyn FnMut(usize),
) -> Result<usize> {
    let mut after = 0;
    let mut total = 0;
    while !cancel() {
        let candidates = lib.face_candidates(after, BATCH, version)?;
        let Some(last) = candidates.last() else {
            break;
        };
        after = last.id;
        let found = detect_batch(&candidates, cache, workers, detect, cancel);
        let batch: Vec<(FaceCandidate, Vec<Detection>)> = candidates
            .into_iter()
            .zip(found)
            .filter_map(|(candidate, faces)| Some((candidate, faces?)))
            .collect();
        let written = lib.write_face_batch(&batch, version)?;
        total += written;
        on_batch(written);
    }
    Ok(total)
}

/// One answer per candidate, in order: the faces found, or `None` for a photo not looked
/// at (its preview unreadable, or the pass cancelled before its turn).
fn detect_batch(
    candidates: &[FaceCandidate],
    cache: &ThumbCache,
    workers: usize,
    detect: &Detect<'_>,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Vec<Option<Vec<Detection>>> {
    let next = AtomicUsize::new(0);
    let found: Mutex<Vec<Option<Vec<Detection>>>> = Mutex::new(vec![None; candidates.len()]);
    std::thread::scope(|scope| {
        for _ in 0..workers.clamp(1, candidates.len().max(1)) {
            scope.spawn(|| {
                loop {
                    if cancel() {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(candidate) = candidates.get(i) else {
                        return;
                    };
                    let faces = detect_one(candidate, cache, detect);
                    found.lock().unwrap_or_else(|e| e.into_inner())[i] = faces;
                }
            });
        }
    });
    found.into_inner().unwrap_or_else(|e| e.into_inner())
}

fn detect_one(
    candidate: &FaceCandidate,
    cache: &ThumbCache,
    detect: &Detect<'_>,
) -> Option<Vec<Detection>> {
    // A candidate's thumbnail is `Ready`, which is set only once its files exist, and they
    // are renamed into place, so a half-written one is not the cause. A removed file (the
    // cache cleared, or collected after the row moved on) is: left a candidate, it is
    // detected once the preview is back. A file libwebp refuses is permanent - `is_complete`
    // only checks that it exists, so it is never re-rendered - and costs one failed read
    // per pass. The look-alike pass leaves an unreadable thumbnail the same way.
    let preview = match cache.read(candidate.thumb_key, ThumbSize::Preview) {
        Ok(preview) => preview,
        Err(err) => {
            tracing::debug!(id = candidate.id, %err, "could not read a preview to detect faces");
            return None;
        }
    };
    // tract is Rust and unwinds, so a photo it panics on costs that photo. It is written
    // as looked-at with no faces: the same picture would panic on every pass.
    match catch_unwind(AssertUnwindSafe(|| detect(&preview))) {
        Ok(Ok(faces)) => Some(faces),
        Ok(Err(err)) => {
            tracing::warn!(id = candidate.id, %err, "face detection failed on a photo");
            Some(Vec::new())
        }
        Err(_) => {
            tracing::warn!(id = candidate.id, "face detection panicked on a photo");
            Some(Vec::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::face_detect::{DETECTOR_VERSION, Rect};
    use crate::media::ThumbState;
    use crate::testutil::{new_item, seed_folder, temp_library};
    use image::RgbImage;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const V: i64 = DETECTOR_VERSION;

    struct Fixture {
        _dir: tempfile::TempDir,
        lib: Library,
        cache: ThumbCache,
        ids: Vec<i64>,
    }

    /// `n` photos, thumbnails ready and cached, the setting on.
    fn fixture(n: usize) -> Fixture {
        let (dir, lib) = temp_library();
        let cache = ThumbCache::new(dir.path().join("thumbs"));
        let (_, folder) = seed_folder(&lib, dir.path());
        let items: Vec<_> = (0..n)
            .map(|i| new_item(folder, &format!("{}/{i}.jpg", dir.path().display()), 1))
            .collect();
        let ids = lib.insert_items(&items).unwrap();
        lib.set_face_detection(true).unwrap();
        for id in &ids {
            lib.set_thumb_state(*id, ThumbState::Ready, None).unwrap();
        }
        let picture = DynamicImage::ImageRgb8(RgbImage::new(64, 48));
        for c in lib.face_candidates(0, n, V).unwrap() {
            cache.store(c.thumb_key, &picture, &picture).unwrap();
        }
        Fixture {
            _dir: dir,
            lib,
            cache,
            ids,
        }
    }

    fn one_face(_: &DynamicImage) -> Result<Vec<Detection>> {
        Ok(vec![Detection {
            rect: Rect {
                left: 0.1,
                top: 0.1,
                right: 0.2,
                bottom: 0.2,
            },
            landmarks: [(0.0, 0.0); 5],
            score: 0.9,
        }])
    }

    fn never() -> bool {
        false
    }

    /// Photos that have a stored face, counted through the public reader.
    fn faces(f: &Fixture) -> i64 {
        f.ids
            .iter()
            .map(|id| f.lib.item_detected_faces(*id).unwrap().len() as i64)
            .sum()
    }

    /// More photos than a batch, so the loop pages: every one is detected, once.
    #[test]
    fn every_candidate_is_detected_once() {
        let f = fixture(BATCH + 5);
        let calls = AtomicUsize::new(0);
        let detect = |img: &DynamicImage| {
            calls.fetch_add(1, Ordering::SeqCst);
            one_face(img)
        };
        let mut batches = Vec::new();
        let written = run(&f.lib, &f.cache, V, 3, &detect, &never, &mut |n| {
            batches.push(n)
        })
        .unwrap();
        assert_eq!(written, BATCH + 5);
        assert_eq!(batches, [BATCH, 5]);
        assert_eq!(calls.load(Ordering::SeqCst), BATCH + 5);
        assert_eq!(faces(&f), (BATCH + 5) as i64);
        assert!(f.lib.face_candidates(0, 10, V).unwrap().is_empty());
    }

    /// A preview that is not in the cache is skipped and the photo stays a candidate - and
    /// the pass ends all the same, rather than asking for it again for ever.
    #[test]
    fn a_missing_preview_is_skipped_and_stays_a_candidate() {
        let f = fixture(3);
        let gone = f.lib.face_candidates(0, 10, V).unwrap()[1].clone();
        std::fs::remove_file(f.cache.path_for(gone.thumb_key, ThumbSize::Preview)).unwrap();
        let written = run(&f.lib, &f.cache, V, 1, &one_face, &never, &mut |_| {}).unwrap();
        assert_eq!(written, 2);
        let left = f.lib.face_candidates(0, 10, V).unwrap();
        assert_eq!(left.iter().map(|c| c.id).collect::<Vec<_>>(), [gone.id]);
    }

    /// A photo the detector panics on is written as looked-at with no faces, so it is not
    /// tried again on every pass, and the photos around it are detected.
    #[test]
    fn a_panic_in_the_detector_costs_one_photo() {
        let f = fixture(3);
        let calls = AtomicUsize::new(0);
        let detect = |img: &DynamicImage| {
            if calls.fetch_add(1, Ordering::SeqCst) == 1 {
                panic!("a photo the model cannot take");
            }
            one_face(img)
        };
        let written = run(&f.lib, &f.cache, V, 1, &detect, &never, &mut |_| {}).unwrap();
        assert_eq!(written, 3);
        assert_eq!(faces(&f), 2);
        assert!(f.lib.face_candidates(0, 10, V).unwrap().is_empty());
    }

    #[test]
    fn an_error_from_the_detector_costs_one_photo() {
        let f = fixture(2);
        let calls = AtomicUsize::new(0);
        let detect = |img: &DynamicImage| {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(crate::Error::FaceModel("no".into()));
            }
            one_face(img)
        };
        assert_eq!(
            run(&f.lib, &f.cache, V, 1, &detect, &never, &mut |_| {}).unwrap(),
            2
        );
        assert_eq!(faces(&f), 1);
    }

    /// Cancelled before it starts, the pass reads nothing and writes nothing.
    #[test]
    fn a_cancelled_pass_does_nothing() {
        let f = fixture(2);
        let calls = AtomicUsize::new(0);
        let detect = |img: &DynamicImage| {
            calls.fetch_add(1, Ordering::SeqCst);
            one_face(img)
        };
        assert_eq!(
            run(&f.lib, &f.cache, V, 2, &detect, &|| true, &mut |_| {}).unwrap(),
            0
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(f.lib.face_candidates(0, 10, V).unwrap().len(), 2);
    }

    /// Cancelled in the middle, the pass stops at the photo it is on.
    #[test]
    fn cancelling_stops_between_photos() {
        let f = fixture(BATCH);
        let calls = AtomicUsize::new(0);
        let detect = |img: &DynamicImage| {
            calls.fetch_add(1, Ordering::SeqCst);
            one_face(img)
        };
        let cancel = || calls.load(Ordering::SeqCst) >= 3;
        run(&f.lib, &f.cache, V, 1, &detect, &cancel, &mut |_| {}).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    /// Cancelled between batches, the pass stops there: it does not page on through the
    /// rest of the library asking for batches it will not detect.
    #[test]
    fn a_cancel_between_batches_ends_the_pass() {
        let f = fixture(BATCH + 5);
        let calls = AtomicUsize::new(0);
        let detect = |img: &DynamicImage| {
            calls.fetch_add(1, Ordering::SeqCst);
            one_face(img)
        };
        let cancel = || calls.load(Ordering::SeqCst) >= BATCH;
        let mut batches = Vec::new();
        let written = run(&f.lib, &f.cache, V, 1, &detect, &cancel, &mut |n| {
            batches.push(n)
        })
        .unwrap();
        assert_eq!(written, BATCH);
        assert_eq!(batches, [BATCH]);
    }

    #[test]
    fn an_empty_library_is_a_pass_with_nothing_to_do() {
        let f = fixture(0);
        let mut batches = 0;
        assert_eq!(
            run(&f.lib, &f.cache, V, 4, &one_face, &never, &mut |_| {
                batches += 1
            })
            .unwrap(),
            0
        );
        assert_eq!(batches, 0);
        assert!(f.ids.is_empty());
    }

    #[test]
    fn workers_are_between_one_and_four() {
        assert!((1..=4).contains(&workers()));
    }
}
