//! One pass over the photos the detector has not looked at: read each one's cached
//! preview, detect, write. The engine owns the thread, the triggers and the progress; this
//! is the loop, here so it can be run against a library and a cache without an engine.

use super::Detection;
use crate::library::{FaceCandidate, Library};
use crate::thumbs::{ThumbCache, ThumbSize};
use crate::{Error, Result};
use image::DynamicImage;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Photos per write. One transaction a batch: small enough that a quit loses under a
/// minute of work, large enough that the writer is not taken per photo.
pub const BATCH: usize = 64;

/// How many photos a batch must have *detected*, every one of them failing, before the
/// batch is treated as a fault of the detector rather than of those photos.
///
/// Why there is a floor at all: a photo that fails is written as looked-at with no faces
/// so that it is not retried on every pass. With a floor of one, a single bad photo that
/// happens to be alone in a final batch would be "every photo of its batch" and never be
/// marked, and would be retried - and trip the breaker - on every pass, which is exactly
/// what marking a failed photo exists to prevent. Why 8 and not the whole batch: a small
/// library, or the tail of a large one, must still be able to trip it. The gap is
/// deliberate and real: fewer than 8 photos, all failing, are still marked.
///
/// Each later trigger of a tripped pass loads the model and fails one batch again, because
/// nothing was marked. That is the intended cost: one batch per scan end or thumbnail
/// drain, with no retry limit, flag or setting.
pub(crate) const BREAKER_FLOOR: usize = 8;

/// What finds the faces in a preview: `Detector::detect`, or a test's stand-in.
pub type Detect<'a> = dyn Fn(&DynamicImage) -> Result<Vec<Detection>> + Sync + 'a;

/// What hands over a candidate's preview: the cache, or a test's stand-in.
type Read<'a> = dyn Fn(&FaceCandidate) -> Result<DynamicImage> + Sync + 'a;

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
///
/// **A batch in which every detected photo failed, and at least [`BREAKER_FLOOR`] did, is
/// not written** and ends the pass with an error: a fault that hits every photo would
/// otherwise mark the whole library as checked with no faces. Skipped photos count
/// neither way. Earlier batches stay written.
pub fn run(
    lib: &Library,
    cache: &ThumbCache,
    version: i64,
    workers: usize,
    detect: &Detect<'_>,
    cancel: &(dyn Fn() -> bool + Sync),
    on_batch: &mut dyn FnMut(usize),
) -> Result<usize> {
    let read = |candidate: &FaceCandidate| cache.read(candidate.thumb_key, ThumbSize::Preview);
    let mut after = 0;
    let mut total = 0;
    while !cancel() {
        let candidates = lib.face_candidates(after, BATCH, version)?;
        let Some(last) = candidates.last() else {
            break;
        };
        after = last.id;
        let found = detect_batch(&candidates, &read, workers, detect, cancel);
        let failed = found
            .iter()
            .filter(|o| matches!(o, Outcome::Failed))
            .count();
        let detected = failed
            + found
                .iter()
                .filter(|o| matches!(o, Outcome::Faces(_)))
                .count();
        if breaker_trips(detected, failed) {
            // Before the write, so nothing of this batch is marked and every photo in it
            // stays a candidate for the next trigger. Earlier batches stay written.
            return Err(Error::FaceModel(format!(
                "face detection failed on {failed} photos in a row; nothing was marked"
            )));
        }
        let batch: Vec<(FaceCandidate, Vec<Detection>)> = candidates
            .into_iter()
            .zip(found)
            .filter_map(|(candidate, outcome)| match outcome {
                Outcome::Skipped => None,
                Outcome::Faces(faces) => Some((candidate, faces)),
                Outcome::Failed => Some((candidate, Vec::new())),
            })
            .collect();
        let written = lib.write_face_batch(&batch, version)?;
        total += written;
        on_batch(written);
    }
    Ok(total)
}

/// What happened to one photo.
#[derive(Clone)]
enum Outcome {
    /// Not detected: its preview could not be read, or the pass was cancelled before its
    /// turn. Neither a success nor a failure, and it is not written.
    Skipped,
    /// Detected; the faces found, possibly none.
    Faces(Vec<Detection>),
    /// The detector returned an error or panicked. Written as looked-at with no faces,
    /// unless [`breaker_trips`].
    Failed,
}

/// Whether a batch is a fault of the detector: every photo detected in it failed, and
/// there were at least [`BREAKER_FLOOR`] of them. Written so that all the photos failing
/// is a systemic fault (the model failing on this machine) and not a run of bad photos,
/// which would be marked and left alone.
fn breaker_trips(detected: usize, failed: usize) -> bool {
    failed == detected && failed >= BREAKER_FLOOR
}

/// One outcome per candidate, in order.
fn detect_batch(
    candidates: &[FaceCandidate],
    read: &Read<'_>,
    workers: usize,
    detect: &Detect<'_>,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Vec<Outcome> {
    let next = AtomicUsize::new(0);
    let found: Mutex<Vec<Outcome>> = Mutex::new(vec![Outcome::Skipped; candidates.len()]);
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
                    let outcome = detect_one(candidate, read, detect);
                    found.lock().unwrap_or_else(|e| e.into_inner())[i] = outcome;
                }
            });
        }
    });
    found.into_inner().unwrap_or_else(|e| e.into_inner())
}

fn detect_one(candidate: &FaceCandidate, read: &Read<'_>, detect: &Detect<'_>) -> Outcome {
    // A candidate's thumbnail is `Ready`, which is set only once its files exist, and they
    // are renamed into place, so a half-written one is not the cause. A removed file (the
    // cache cleared, or collected after the row moved on) is: left a candidate, it is
    // detected once the preview is back. A file libwebp refuses is permanent - `is_complete`
    // only checks that it exists, so it is never re-rendered - and costs one failed read
    // per pass. The look-alike pass leaves an unreadable thumbnail the same way.
    //
    // The read is guarded as the detection is, apart from it because the two end
    // differently: decoding goes through a crate that is not ours, and a panic there,
    // unguarded, unwinds the pass's thread - no batch after it, no last progress event,
    // and the same on every pass, each of which comes to the same file. A file that
    // panics is treated as one that cannot be read: skipped, and still a candidate.
    let preview = match catch_unwind(AssertUnwindSafe(|| read(candidate))) {
        Ok(Ok(preview)) => preview,
        Ok(Err(err)) => {
            tracing::debug!(id = candidate.id, %err, "could not read a preview to detect faces");
            return Outcome::Skipped;
        }
        Err(_) => {
            tracing::warn!(
                id = candidate.id,
                "reading a preview to detect faces panicked"
            );
            return Outcome::Skipped;
        }
    };
    // tract is Rust and unwinds, so a photo it panics on costs that photo. It is written
    // as looked-at with no faces (the caller does that, unless the whole batch failed):
    // the same picture would panic on every pass.
    match catch_unwind(AssertUnwindSafe(|| detect(&preview))) {
        Ok(Ok(faces)) => Outcome::Faces(faces),
        Ok(Err(err)) => {
            tracing::warn!(id = candidate.id, %err, "face detection failed on a photo");
            Outcome::Failed
        }
        Err(_) => {
            tracing::warn!(id = candidate.id, "face detection panicked on a photo");
            Outcome::Failed
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
        fixture_with(n, |_| RgbImage::new(64, 48))
    }

    /// The same, each photo's preview made by `preview` from its place in id order. (The
    /// cache does not write over a file it has, so a test cannot swap one in afterwards.)
    fn fixture_with(n: usize, preview: impl Fn(usize) -> RgbImage) -> Fixture {
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
        for (i, c) in lib.face_candidates(0, n, V).unwrap().iter().enumerate() {
            let picture = DynamicImage::ImageRgb8(preview(i));
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

    /// A preview whose decode panics is skipped like one that cannot be read: the photos
    /// around it are detected, it is not, and the pass's thread survives it.
    #[test]
    fn a_panic_reading_a_preview_skips_the_photo() {
        let f = fixture(3);
        let candidates = f.lib.face_candidates(0, 10, V).unwrap();
        let bad = candidates[1].id;
        let read = |c: &FaceCandidate| {
            if c.id == bad {
                panic!("a preview the decoder cannot take");
            }
            f.cache.read(c.thumb_key, ThumbSize::Preview)
        };
        let found = detect_batch(&candidates, &read, 1, &one_face, &never);
        assert_eq!(
            found
                .iter()
                .map(|o| match o {
                    Outcome::Faces(f) => Some(f.len()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [Some(1), None, Some(1)]
        );
    }

    /// Each photo gets the faces found in its own preview. The workers finish in an order
    /// that is not the candidates' - here the earliest photos are the slowest - so an
    /// answer filed by when it arrived, rather than by whose it is, lands on another photo.
    /// Every preview has a width of its own and the stand-in answers with it.
    #[test]
    fn each_photo_is_given_its_own_faces() {
        let width = |i: usize| 16 + 4 * i as u32;
        let f = fixture_with(12, |i| RgbImage::new(width(i), 8));
        let candidates = f.lib.face_candidates(0, 100, V).unwrap();
        assert_eq!(candidates.len(), 12);
        let detect = |img: &DynamicImage| {
            let slowest_first = (width(12) - img.width()) as u64;
            std::thread::sleep(std::time::Duration::from_millis(slowest_first));
            let mut faces = one_face(img)?;
            faces[0].rect.left = img.width() as f64 / 1000.0;
            Ok(faces)
        };
        assert_eq!(
            run(&f.lib, &f.cache, V, 3, &detect, &never, &mut |_| {}).unwrap(),
            12
        );
        for (i, c) in candidates.iter().enumerate() {
            let stored = f.lib.item_detected_faces(c.id).unwrap();
            assert_eq!(stored.len(), 1);
            assert_eq!(stored[0].left, width(i) as f64 / 1000.0, "photo {i}");
        }
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

    fn failing(_: &DynamicImage) -> Result<Vec<Detection>> {
        Err(Error::FaceModel("no".into()))
    }

    fn panicking(_: &DynamicImage) -> Result<Vec<Detection>> {
        panic!("a model that cannot run");
    }

    /// Runs the pass and gives back its result and what `on_batch` was told.
    fn run_counting(f: &Fixture, detect: &Detect<'_>) -> (Result<usize>, Vec<usize>) {
        let mut batches = Vec::new();
        let r = run(&f.lib, &f.cache, V, 3, detect, &never, &mut |n| {
            batches.push(n)
        });
        (r, batches)
    }

    fn untouched(f: &Fixture) -> bool {
        f.lib.face_candidates(0, 1000, V).unwrap().len() == f.ids.len() && faces(f) == 0
    }

    #[test]
    fn the_decision_is_every_detected_photo_failing_and_at_least_the_floor() {
        // Literals: the floor itself is probed against these.
        assert!(breaker_trips(8, 8));
        assert!(breaker_trips(BATCH, BATCH));
        assert!(!breaker_trips(7, 7));
        assert!(!breaker_trips(0, 0));
        assert!(!breaker_trips(BATCH, BATCH - 1));
        assert!(!breaker_trips(9, 8));
    }

    /// A fault on every photo of a full batch marks nothing and ends the pass with an
    /// error; the batch is not reported.
    #[test]
    fn a_batch_where_every_photo_fails_is_not_written() {
        let f = fixture(BATCH);
        let (r, batches) = run_counting(&f, &failing);
        let err = r.unwrap_err().to_string();
        assert!(err.contains("nothing was marked"), "{err}");
        assert!(batches.is_empty());
        assert!(untouched(&f));
    }

    /// The floor: eight failures trip it, seven are marked like any bad photos.
    #[test]
    fn the_floor_is_eight_photos() {
        let f = fixture(BREAKER_FLOOR);
        let (r, batches) = run_counting(&f, &failing);
        assert!(r.is_err());
        assert!(batches.is_empty());
        assert!(untouched(&f));

        let f = fixture(BREAKER_FLOOR - 1);
        let (r, batches) = run_counting(&f, &failing);
        assert_eq!(r.unwrap(), BREAKER_FLOOR - 1);
        assert_eq!(batches, [BREAKER_FLOOR - 1]);
        assert!(f.lib.face_candidates(0, 100, V).unwrap().is_empty());
    }

    /// One success in the batch means the detector works: the failures are marked.
    #[test]
    fn a_batch_with_one_success_is_written_as_before() {
        let f = fixture(BATCH);
        let calls = AtomicUsize::new(0);
        let detect = |img: &DynamicImage| {
            if calls.fetch_add(1, Ordering::SeqCst) == 17 {
                return one_face(img);
            }
            failing(img)
        };
        let (r, batches) = run_counting(&f, &detect);
        assert_eq!(r.unwrap(), BATCH);
        assert_eq!(batches, [BATCH]);
        assert_eq!(faces(&f), 1);
        assert!(f.lib.face_candidates(0, 1000, V).unwrap().is_empty());
    }

    /// A photo detected without error and with no faces is a success.
    #[test]
    fn a_detection_with_no_faces_is_a_success() {
        let f = fixture(BATCH);
        let calls = AtomicUsize::new(0);
        let detect = |_: &DynamicImage| {
            if calls.fetch_add(1, Ordering::SeqCst) == 3 {
                return Ok(Vec::new());
            }
            failing(&DynamicImage::new_rgb8(1, 1))
        };
        let (r, _) = run_counting(&f, &detect);
        assert_eq!(r.unwrap(), BATCH);
        assert!(f.lib.face_candidates(0, 1000, V).unwrap().is_empty());
    }

    /// A panic is a failure like an error.
    #[test]
    fn panics_count_as_failures() {
        let f = fixture(BATCH);
        let (r, batches) = run_counting(&f, &panicking);
        assert!(r.is_err());
        assert!(batches.is_empty());
        assert!(untouched(&f));

        // Half by panic, half by error.
        let f = fixture(BATCH);
        let calls = AtomicUsize::new(0);
        let detect = |img: &DynamicImage| {
            if calls.fetch_add(1, Ordering::SeqCst).is_multiple_of(2) {
                panicking(img)
            } else {
                failing(img)
            }
        };
        assert!(run_counting(&f, &detect).0.is_err());
        assert!(untouched(&f));
    }

    /// A photo whose preview cannot be read is neither: eight failures among unreadable
    /// previews still trip, and seven do not.
    #[test]
    fn skipped_photos_count_neither_way() {
        for (failures, trips) in [(BREAKER_FLOOR, true), (BREAKER_FLOOR - 1, false)] {
            let f = fixture(failures + 5);
            let candidates = f.lib.face_candidates(0, 100, V).unwrap();
            for gone in &candidates[..5] {
                std::fs::remove_file(f.cache.path_for(gone.thumb_key, ThumbSize::Preview)).unwrap();
            }
            let (r, _) = run_counting(&f, &failing);
            assert_eq!(r.is_err(), trips, "{failures} failures");
            let left = f.lib.face_candidates(0, 100, V).unwrap().len();
            // Tripped: all stay candidates. Not: only the unreadable five do.
            assert_eq!(left, if trips { failures + 5 } else { 5 });
        }
    }

    /// A pass cut short by cancel does not count the photos it never reached as
    /// successes: the eight it did reach fail and trip, though the rest are untried.
    #[test]
    fn photos_not_reached_do_not_count() {
        let f = fixture(BATCH);
        let calls = AtomicUsize::new(0);
        let detect = |img: &DynamicImage| {
            calls.fetch_add(1, Ordering::SeqCst);
            failing(img)
        };
        let cancel = || calls.load(Ordering::SeqCst) >= BREAKER_FLOOR;
        let r = run(&f.lib, &f.cache, V, 1, &detect, &cancel, &mut |_| {});
        assert!(r.is_err());
        assert!(untouched(&f));
    }

    /// A batch that trips leaves the one before it written.
    #[test]
    fn an_earlier_batch_stays_written() {
        let f = fixture(BATCH + BREAKER_FLOOR);
        let calls = AtomicUsize::new(0);
        let detect = |img: &DynamicImage| {
            if calls.fetch_add(1, Ordering::SeqCst) < BATCH {
                one_face(img)
            } else {
                failing(img)
            }
        };
        let (r, batches) = run_counting(&f, &detect);
        assert!(r.is_err());
        assert_eq!(batches, [BATCH]);
        assert_eq!(faces(&f), BATCH as i64);
        assert_eq!(
            f.lib.face_candidates(0, 1000, V).unwrap().len(),
            BREAKER_FLOOR
        );
    }

    #[test]
    fn workers_are_between_one_and_four() {
        assert!((1..=4).contains(&workers()));
    }
}
