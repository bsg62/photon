//! One pass over the faces the embedder has not looked at: per photo, read its cached
//! preview once, embed each of its faces, write. The engine owns the thread, the triggers
//! and the progress; this is the loop, here so it can be run against a library and a cache
//! without an engine. It has `face_detect::pass`'s shape, and is not a generic loop shared
//! with it: a candidate here is a photo with several faces to embed, and an outcome is one
//! per face rather than one per photo.

use super::{Embedding, FaceBox};
use crate::face_detect::pass::{BATCH, BREAKER_FLOOR};
use crate::library::{EmbedCandidate, Library};
use crate::thumbs::{ThumbCache, ThumbSize};
use crate::{Error, Result};
use image::DynamicImage;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// What makes one face's vector: `Embedder::embed`, or a test's stand-in.
pub type Embed<'a> = dyn Fn(&DynamicImage, &FaceBox) -> Result<Option<Embedding>> + Sync + 'a;

/// What hands over a candidate's preview: the cache, or a test's stand-in.
type Read<'a> = dyn Fn(&EmbedCandidate) -> Result<DynamicImage> + Sync + 'a;

/// Embeds every candidate's faces, in batches of [`BATCH`] photos. Returns how many faces
/// were written; `on_batch` is told each batch's count as it lands.
///
/// **Pages by id**, as the detection pass does: a photo whose preview cannot be read is
/// skipped without being written, so it is still a candidate, and asked for again from the
/// start it would be handed back for ever. The next pass asks again.
///
/// **A batch in which every face the model was asked about failed, and at least
/// [`BREAKER_FLOOR`] did, is not written** and ends the pass with an error, for the
/// detection pass's reason: a fault that hits every face would otherwise mark the whole
/// library as embedded with no vectors. A face too small is not asked about - the embedder
/// answers it without running the model - so, like a skipped photo, it counts neither way.
/// Earlier batches stay written.
pub fn run(
    lib: &Library,
    cache: &ThumbCache,
    version: i64,
    workers: usize,
    embed: &Embed<'_>,
    cancel: &(dyn Fn() -> bool + Sync),
    on_batch: &mut dyn FnMut(usize),
) -> Result<usize> {
    let read = |candidate: &EmbedCandidate| cache.read(candidate.thumb_key, ThumbSize::Preview);
    let mut after = 0;
    let mut total = 0;
    while !cancel() {
        let candidates = lib.embed_candidates(after, BATCH, version)?;
        let Some(last) = candidates.last() else {
            break;
        };
        after = last.item_id;
        let found = embed_batch(&candidates, &read, workers, embed, cancel);
        let (mut embedded, mut failed) = (0, 0);
        for face in found.iter().flatten().flatten() {
            match face {
                Face::Embedded(_) => embedded += 1,
                Face::Failed => failed += 1,
                Face::TooSmall => {}
            }
        }
        if breaker_trips(embedded + failed, failed) {
            // Before the write, so nothing of this batch is marked and every face in it
            // stays a candidate for the next trigger. Earlier batches stay written.
            return Err(Error::FaceModel(format!(
                "all {failed} faces embedded in a batch failed; nothing was marked"
            )));
        }
        let batch: Vec<_> = candidates
            .into_iter()
            .zip(found)
            .filter_map(|(candidate, outcome)| {
                let faces = outcome?;
                let vectors = candidate
                    .faces
                    .iter()
                    .zip(faces)
                    .map(|((id, _), face)| match face {
                        Face::Embedded(vector) => (*id, Some(*vector)),
                        Face::TooSmall | Face::Failed => (*id, None),
                    })
                    .collect();
                Some((candidate, vectors))
            })
            .collect();
        let written = lib.write_embeddings(&batch, version)?;
        total += written;
        on_batch(written);
    }
    Ok(total)
}

/// What happened to one face. A photo that was not embedded at all (its preview could not
/// be read, or the pass was cancelled before its turn) is a `None` where this would be: not
/// written, neither a success nor a failure.
#[derive(Clone)]
enum Face {
    /// Boxed: a vector is 512 bytes and the other two variants are nothing.
    Embedded(Box<Embedding>),
    /// Narrower than the embedder's minimum: marked, with no vector, and not counted.
    TooSmall,
    /// The embedder returned an error or panicked. Marked with no vector, unless
    /// [`breaker_trips`].
    Failed,
}

/// One photo's faces, in the candidate's order; `None` for a photo not embedded.
type Outcome = Option<Vec<Face>>;

/// Whether a batch is not to be written, from two counts: the faces the model was asked
/// about and how many of those failed. It trips when they are equal and at least
/// [`BREAKER_FLOOR`]. It only counts: it assumes that so many failures with no success is
/// the model failing on this machine rather than that many bad faces, which it cannot tell
/// apart.
fn breaker_trips(asked: usize, failed: usize) -> bool {
    failed == asked && failed >= BREAKER_FLOOR
}

/// One outcome per candidate, in order.
fn embed_batch(
    candidates: &[EmbedCandidate],
    read: &Read<'_>,
    workers: usize,
    embed: &Embed<'_>,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Vec<Outcome> {
    let next = AtomicUsize::new(0);
    let found: Mutex<Vec<Outcome>> = Mutex::new(vec![None; candidates.len()]);
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
                    let outcome = embed_one(candidate, read, embed);
                    found.lock().unwrap_or_else(|e| e.into_inner())[i] = outcome;
                }
            });
        }
    });
    found.into_inner().unwrap_or_else(|e| e.into_inner())
}

fn embed_one(candidate: &EmbedCandidate, read: &Read<'_>, embed: &Embed<'_>) -> Outcome {
    // The read is guarded apart from the embedding, as in the detection pass, and for its
    // reasons: a removed cache file comes back and is embedded then; a file libwebp refuses
    // costs one failed read per pass; a decode that panics must not unwind the pass's
    // thread. Either way the photo is skipped and its faces stay candidates.
    let preview = match catch_unwind(AssertUnwindSafe(|| read(candidate))) {
        Ok(Ok(preview)) => preview,
        Ok(Err(err)) => {
            tracing::debug!(id = candidate.item_id, %err, "could not read a preview to embed faces");
            return None;
        }
        Err(_) => {
            tracing::warn!(
                id = candidate.item_id,
                "reading a preview to embed faces panicked"
            );
            return None;
        }
    };
    // tract is Rust and unwinds, so a face it panics on costs that face: marked with no
    // vector by the caller (unless the whole batch failed), since the same face would panic
    // on every pass. Each face is guarded alone so one does not cost its photo's others.
    let faces = candidate
        .faces
        .iter()
        .map(
            |(id, face)| match catch_unwind(AssertUnwindSafe(|| embed(&preview, face))) {
                Ok(Ok(Some(vector))) => Face::Embedded(Box::new(vector)),
                Ok(Ok(None)) => Face::TooSmall,
                Ok(Err(err)) => {
                    tracing::warn!(face = id, %err, "embedding a face failed");
                    Face::Failed
                }
                Err(_) => {
                    tracing::warn!(face = id, "embedding a face panicked");
                    Face::Failed
                }
            },
        )
        .collect();
    Some(faces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::face_detect::{Detection, Rect};
    use crate::face_embed::{DIM, EMBEDDER_VERSION, from_blob};
    use crate::media::ThumbState;
    use crate::testutil::{new_item, seed_folder, temp_library};
    use image::RgbImage;

    const V: i64 = EMBEDDER_VERSION;

    struct Fixture {
        _dir: tempfile::TempDir,
        lib: Library,
        cache: ThumbCache,
    }

    /// `n` photos with two detected faces each, previews cached, the setting on.
    fn fixture(n: usize) -> Fixture {
        fixture_with(n, |_| 2, |_| RgbImage::new(64, 48))
    }

    /// The same, each photo with `faces(i)` faces and a preview made by `preview(i)`, `i`
    /// being its place in id order. (The cache does not write over a file it has, so a test
    /// cannot swap a preview in afterwards.) A photo's faces are at `left` 0.1, 0.3, ...
    fn fixture_with(
        n: usize,
        faces: impl Fn(usize) -> usize,
        preview: impl Fn(usize) -> RgbImage,
    ) -> Fixture {
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
        let detected = lib.face_candidates(0, n, crate::face_detect::DETECTOR_VERSION);
        let detected = detected.unwrap();
        let batch: Vec<_> = detected
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let found = (0..faces(i))
                    .map(|k| detection(0.1 + 0.2 * k as f64))
                    .collect();
                (c.clone(), found)
            })
            .collect();
        lib.write_face_batch(&batch, crate::face_detect::DETECTOR_VERSION)
            .unwrap();
        for (i, c) in detected.iter().enumerate() {
            let picture = DynamicImage::ImageRgb8(preview(i));
            cache.store(c.thumb_key, &picture, &picture).unwrap();
        }
        Fixture {
            _dir: dir,
            lib,
            cache,
        }
    }

    fn detection(left: f64) -> Detection {
        Detection {
            rect: Rect {
                left,
                top: 0.2,
                right: left + 0.1,
                bottom: 0.4,
            },
            landmarks: [(0.1, 0.2), (0.3, 0.4), (0.5, 0.6), (0.7, 0.8), (0.9, 1.0)],
            score: 0.9,
        }
    }

    /// A vector that says whose it is: the picture's width and the face's `left` in
    /// thousandths.
    fn marked(img: &DynamicImage, face: &FaceBox) -> Embedding {
        let mut e = [0f32; DIM];
        e[0] = img.width() as f32;
        e[1] = (face.rect.left * 1000.0).round() as f32;
        e
    }

    fn one_vector(img: &DynamicImage, face: &FaceBox) -> Result<Option<Embedding>> {
        Ok(Some(marked(img, face)))
    }

    fn never() -> bool {
        false
    }

    /// Every face row as `(item_id, left, embedding_version, vector)`, read straight from
    /// the database: the library has no reader of vectors outside the grouping.
    #[allow(clippy::type_complexity)]
    fn stored(f: &Fixture) -> Vec<(i64, f64, Option<i64>, Option<Embedding>)> {
        let conn = rusqlite::Connection::open_with_flags(
            f.lib.path(),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT item_id, left, embedding_version, embedding FROM detected_faces ORDER BY id",
            )
            .unwrap();
        stmt.query_map([], |r| {
            let blob: Option<Vec<u8>> = r.get(3)?;
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                blob.map(|b| from_blob(&b).unwrap()),
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
    }

    fn vectors(f: &Fixture) -> usize {
        stored(f).iter().filter(|r| r.3.is_some()).count()
    }

    fn marked_as_looked_at(f: &Fixture) -> usize {
        stored(f).iter().filter(|r| r.2 == Some(V)).count()
    }

    fn candidates(f: &Fixture) -> Vec<EmbedCandidate> {
        f.lib.embed_candidates(0, 1000, V).unwrap()
    }

    /// Runs the pass and gives back its result and what `on_batch` was told.
    fn run_counting(f: &Fixture, embed: &Embed<'_>) -> (Result<usize>, Vec<usize>) {
        let mut batches = Vec::new();
        let r = run(&f.lib, &f.cache, V, 3, embed, &never, &mut |n| {
            batches.push(n);
            // A pass that is handed the same photo again never ends.
            assert!(batches.len() < 50, "the pass did not end");
        });
        (r, batches)
    }

    /// The breaker's own error, not an `Err` from somewhere else in the pass.
    fn tripped<T: std::fmt::Debug>(r: Result<T>) -> bool {
        matches!(r, Err(Error::FaceModel(m)) if m.contains("nothing was marked"))
    }

    fn untouched(f: &Fixture) -> bool {
        stored(f).iter().all(|r| r.2.is_none() && r.3.is_none())
            && candidates(f).iter().map(|c| c.faces.len()).sum::<usize>() == stored(f).len()
    }

    fn failing(_: &DynamicImage, _: &FaceBox) -> Result<Option<Embedding>> {
        Err(Error::FaceModel("no".into()))
    }

    /// More photos than a batch, so the loop pages: every face is embedded, once.
    #[test]
    fn every_face_is_embedded_once() {
        let f = fixture(BATCH + 5);
        let calls = AtomicUsize::new(0);
        let embed = |img: &DynamicImage, face: &FaceBox| {
            calls.fetch_add(1, Ordering::SeqCst);
            one_vector(img, face)
        };
        let (r, batches) = run_counting(&f, &embed);
        assert_eq!(r.unwrap(), 2 * (BATCH + 5));
        assert_eq!(batches, [2 * BATCH, 10]);
        assert_eq!(calls.load(Ordering::SeqCst), 2 * (BATCH + 5));
        assert_eq!(vectors(&f), 2 * (BATCH + 5));
        assert!(candidates(&f).is_empty());
    }

    /// A face the embedder says is too small is marked as looked at, with no vector, and
    /// its photo's other face is embedded.
    #[test]
    fn a_face_too_small_is_marked_without_a_vector() {
        let f = fixture(2);
        let embed = |img: &DynamicImage, face: &FaceBox| {
            // The second photo's second face.
            if img.width() == 64 && face.rect.left > 0.2 && face.rect.left < 0.4 {
                return Ok(None);
            }
            one_vector(img, face)
        };
        let (r, _) = run_counting(&f, &embed);
        // Both faces of both photos are marked...
        assert_eq!(r.unwrap(), 4);
        assert_eq!(marked_as_looked_at(&f), 4);
        assert!(candidates(&f).is_empty());
        // ...and the 0.3 face of each photo has no vector, the 0.1 face has one.
        for (_, left, _, vector) in stored(&f) {
            assert_eq!(vector.is_some(), left < 0.2, "face at {left}");
        }
    }

    /// A preview that is not in the cache is skipped and its faces stay candidates - and
    /// the pass ends all the same, rather than asking for the photo again for ever.
    #[test]
    fn a_missing_preview_skips_the_photo() {
        let f = fixture(3);
        let gone = candidates(&f)[1].clone();
        std::fs::remove_file(f.cache.path_for(gone.thumb_key, ThumbSize::Preview)).unwrap();
        let mut batches = 0;
        let written = run(&f.lib, &f.cache, V, 1, &one_vector, &never, &mut |_| {
            batches += 1;
            // A pass that pages from the start is handed the photo again each time.
            assert!(batches < 5, "the pass did not end");
        })
        .unwrap();
        assert_eq!(written, 4);
        let left = candidates(&f);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].item_id, gone.item_id);
        assert_eq!(left[0].faces.len(), 2);
    }

    /// A preview whose decode panics is skipped like one that cannot be read: the photos
    /// around it are embedded, it is not, and the pass's thread survives it.
    #[test]
    fn a_panic_reading_a_preview_skips_the_photo() {
        let f = fixture(3);
        let all = candidates(&f);
        let bad = all[1].item_id;
        let read = |c: &EmbedCandidate| {
            if c.item_id == bad {
                panic!("a preview the decoder cannot take");
            }
            f.cache.read(c.thumb_key, ThumbSize::Preview)
        };
        let found = embed_batch(&all, &read, 1, &one_vector, &never);
        assert_eq!(
            found
                .iter()
                .map(|o| o.as_ref().map(Vec::len))
                .collect::<Vec<_>>(),
            [Some(2), None, Some(2)]
        );
    }

    /// A face the embedder panics on is marked without a vector, so it is not tried again
    /// on every pass, and the faces around it - its photo's other face too - are embedded.
    #[test]
    fn a_panic_costs_one_face() {
        let f = fixture(3);
        let calls = AtomicUsize::new(0);
        let embed = |img: &DynamicImage, face: &FaceBox| {
            if calls.fetch_add(1, Ordering::SeqCst) == 1 {
                panic!("a face the model cannot take");
            }
            one_vector(img, face)
        };
        let written = run(&f.lib, &f.cache, V, 1, &embed, &never, &mut |_| {}).unwrap();
        assert_eq!(written, 6);
        assert_eq!(marked_as_looked_at(&f), 6);
        assert_eq!(vectors(&f), 5);
        assert!(candidates(&f).is_empty());
    }

    /// An error is a failure like a panic, and costs its face alone.
    #[test]
    fn an_error_costs_one_face() {
        let f = fixture(2);
        let calls = AtomicUsize::new(0);
        let embed = |img: &DynamicImage, face: &FaceBox| {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(Error::FaceModel("no".into()));
            }
            one_vector(img, face)
        };
        let (r, _) = run_counting(&f, &embed);
        assert_eq!(r.unwrap(), 4);
        assert_eq!(vectors(&f), 3);
        assert!(candidates(&f).is_empty());
    }

    /// Each face gets the vector made from its own photo and its own box. The workers
    /// finish in an order that is not the candidates' (the earliest photos are the
    /// slowest), so an answer filed by when it arrived, rather than by whose it is, lands
    /// on another photo or face. Every preview has a width of its own and the stand-in
    /// answers with it.
    #[test]
    fn each_photo_gets_its_own_vectors() {
        let width = |i: usize| 16 + 4 * i as u32;
        let f = fixture_with(12, |_| 2, |i| RgbImage::new(width(i), 8));
        let all = candidates(&f);
        assert_eq!(all.len(), 12);
        let embed = |img: &DynamicImage, face: &FaceBox| {
            let slowest_first = (width(12) - img.width()) as u64;
            std::thread::sleep(std::time::Duration::from_millis(slowest_first));
            one_vector(img, face)
        };
        let written = run(&f.lib, &f.cache, V, 3, &embed, &never, &mut |_| {}).unwrap();
        assert_eq!(written, 24);
        let rows = stored(&f);
        for (i, c) in all.iter().enumerate() {
            let mine: Vec<_> = rows.iter().filter(|r| r.0 == c.item_id).collect();
            assert_eq!(mine.len(), 2);
            for (_, left, _, vector) in mine {
                let v = vector.expect("a vector");
                assert_eq!(v[0], width(i) as f32, "photo {i}");
                assert_eq!(v[1], (left * 1000.0).round() as f32, "photo {i}");
            }
        }
    }

    /// Cancelled before it starts, the pass reads nothing and writes nothing.
    #[test]
    fn a_cancelled_pass_does_nothing() {
        let f = fixture(2);
        let calls = AtomicUsize::new(0);
        let embed = |img: &DynamicImage, face: &FaceBox| {
            calls.fetch_add(1, Ordering::SeqCst);
            one_vector(img, face)
        };
        assert_eq!(
            run(&f.lib, &f.cache, V, 2, &embed, &|| true, &mut |_| {}).unwrap(),
            0
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(untouched(&f));
    }

    #[test]
    fn the_decision_is_every_asked_face_failing_and_at_least_the_floor() {
        // Literals: the floor itself is probed against these.
        assert!(breaker_trips(8, 8));
        assert!(breaker_trips(BATCH, BATCH));
        assert!(!breaker_trips(7, 7));
        assert!(!breaker_trips(0, 0));
        assert!(!breaker_trips(BATCH, BATCH - 1));
        assert!(!breaker_trips(9, 8));
    }

    /// A fault on every face of a full batch marks nothing and ends the pass with an
    /// error; the batch is not reported.
    #[test]
    fn a_batch_where_every_face_fails_is_not_written() {
        let f = fixture(BATCH);
        let (r, batches) = run_counting(&f, &failing);
        let err = r.unwrap_err();
        assert!(
            matches!(&err, Error::FaceModel(m) if m.contains(&format!("all {} faces", 2 * BATCH)) && m.contains("nothing was marked")),
            "{err:?}"
        );
        assert!(batches.is_empty());
        assert!(untouched(&f));
    }

    /// The floor: eight failures trip it.
    #[test]
    fn eight_failing_faces_are_not_written() {
        let f = fixture_with(BREAKER_FLOOR, |_| 1, |_| RgbImage::new(64, 48));
        let (r, batches) = run_counting(&f, &failing);
        assert!(tripped(r));
        assert!(batches.is_empty());
        assert!(untouched(&f));
    }

    /// Seven are marked like any bad faces, so a lone bad one is not retried on every pass.
    #[test]
    fn seven_failing_faces_are_written() {
        let f = fixture_with(BREAKER_FLOOR - 1, |_| 1, |_| RgbImage::new(64, 48));
        let (r, batches) = run_counting(&f, &failing);
        assert_eq!(r.unwrap(), BREAKER_FLOOR - 1);
        assert_eq!(batches, [BREAKER_FLOOR - 1]);
        assert_eq!(marked_as_looked_at(&f), BREAKER_FLOOR - 1);
        assert_eq!(vectors(&f), 0);
        assert!(candidates(&f).is_empty());
    }

    /// One success in the batch means the model works: the failures are marked.
    #[test]
    fn a_batch_with_one_success_is_written() {
        let f = fixture(BATCH);
        let calls = AtomicUsize::new(0);
        let embed = |img: &DynamicImage, face: &FaceBox| {
            if calls.fetch_add(1, Ordering::SeqCst) == 17 {
                return one_vector(img, face);
            }
            failing(img, face)
        };
        let (r, batches) = run_counting(&f, &embed);
        assert_eq!(r.unwrap(), 2 * BATCH);
        assert_eq!(batches, [2 * BATCH]);
        assert_eq!(vectors(&f), 1);
        assert!(candidates(&f).is_empty());
    }

    /// A face too small is not one the model was asked about: it is neither a failure nor
    /// a success. Eight failures trip with three of them among the batch, seven do not.
    #[test]
    fn too_small_faces_do_not_count_towards_the_breaker() {
        for (failures, trips) in [(BREAKER_FLOOR, true), (BREAKER_FLOOR - 1, false)] {
            let f = fixture_with(failures + 3, |_| 1, |_| RgbImage::new(64, 48));
            let calls = AtomicUsize::new(0);
            let embed = |_: &DynamicImage, _: &FaceBox| {
                if calls.fetch_add(1, Ordering::SeqCst) < 3 {
                    Ok(None)
                } else {
                    Err(Error::FaceModel("no".into()))
                }
            };
            let (r, _) = run_counting(&f, &embed);
            if trips {
                assert!(tripped(r), "{failures} failures");
                assert!(untouched(&f));
            } else {
                assert_eq!(r.unwrap(), failures + 3, "{failures} failures");
                assert!(candidates(&f).is_empty());
            }
        }
    }

    /// A photo whose preview cannot be read is neither: eight failures among unreadable
    /// previews still trip, and seven do not.
    #[test]
    fn skipped_photos_count_neither_way() {
        for (failures, trips) in [(BREAKER_FLOOR, true), (BREAKER_FLOOR - 1, false)] {
            let f = fixture_with(failures + 5, |_| 1, |_| RgbImage::new(64, 48));
            for gone in &candidates(&f)[..5] {
                std::fs::remove_file(f.cache.path_for(gone.thumb_key, ThumbSize::Preview)).unwrap();
            }
            let (r, _) = run_counting(&f, &failing);
            assert_eq!(tripped(r), trips, "{failures} failures");
            // Tripped: all stay candidates. Not: only the unreadable five do.
            assert_eq!(candidates(&f).len(), if trips { failures + 5 } else { 5 });
        }
    }

    /// A pass cut short by cancel does not count the photos it never reached as
    /// successes: the eight it did reach fail and trip, though the rest are untried.
    #[test]
    fn photos_not_reached_do_not_count() {
        let f = fixture_with(BATCH, |_| 1, |_| RgbImage::new(64, 48));
        let calls = AtomicUsize::new(0);
        let embed = |img: &DynamicImage, face: &FaceBox| {
            calls.fetch_add(1, Ordering::SeqCst);
            failing(img, face)
        };
        let cancel = || calls.load(Ordering::SeqCst) >= BREAKER_FLOOR;
        let r = run(&f.lib, &f.cache, V, 1, &embed, &cancel, &mut |_| {});
        assert!(tripped(r));
        assert!(untouched(&f));
    }

    /// A batch that trips leaves the one before it written.
    #[test]
    fn an_earlier_batch_stays_written() {
        let f = fixture_with(BATCH + BREAKER_FLOOR, |_| 1, |_| RgbImage::new(64, 48));
        let calls = AtomicUsize::new(0);
        let embed = |img: &DynamicImage, face: &FaceBox| {
            if calls.fetch_add(1, Ordering::SeqCst) < BATCH {
                one_vector(img, face)
            } else {
                failing(img, face)
            }
        };
        let (r, batches) = run_counting(&f, &embed);
        assert!(tripped(r));
        assert_eq!(batches, [BATCH]);
        assert_eq!(vectors(&f), BATCH);
        assert_eq!(candidates(&f).len(), BREAKER_FLOOR);
    }
}
