use super::inflight::{CRASH_MESSAGE, DEATHS_TO_FAIL, InFlight};
use super::{Priority, ThumbCache, ThumbQueue, ThumbSize};
use crate::{
    Error, Result,
    edit::Edit,
    library::{Item, Library},
    media::ThumbState,
};
use image::DynamicImage;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::Arc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// The most thumbnail workers photon will run, however many cores the machine has.
///
/// Each worker holds a whole decoded image while it downscales it, and `image`'s default
/// limit allows up to 512 MiB for one decode, so the peak memory of the thumbnail pool is
/// this number times that. Without a cap it grows with the core count: a 32-core machine
/// importing a folder of large scans or panoramas could put ~16 GiB of decode buffers in
/// flight at once. Eight workers saturate the disk on any machine photon runs on, so the
/// cap costs no throughput worth having.
const MAX_WORKERS: usize = 8;

/// How long a suspect's decode waits for `decode_lock` exclusively before backing off.
/// parking_lot's `RwLock` is task-fair, so once the suspect is waiting on `write()`, every
/// later `read()` queues behind that wait too, even for an unrelated photo; backing off after
/// this bound - rather than waiting indefinitely - is what stops a suspect stuck behind one
/// slow ordinary decode from stalling the whole pool behind its own wait. It costs a suspect a
/// retry whenever an *ordinary* decode legitimately takes longer than this (a large AVIF, a
/// panorama): that photo was always going to take a while, so paying for one extra wait cycle
/// on top is cheap, and correct - nothing here can wrongly fail it, only delay it. See
/// `process` and `SUSPECT_BACKOFF_START`, which is what actually keeps a suspect from
/// retrying again immediately once it does back off.
const SUSPECT_WAIT: Duration = Duration::from_secs(1);

/// How long a suspect waits, after failing to get `decode_lock` in time, before it is even
/// eligible to try again - doubling on each further timeout up to [`SUSPECT_BACKOFF_MAX`].
/// `State::push` honours this too: a push for an id currently backing off only raises the
/// priority it will run at, never readmitting it before `not_before` - so nothing that pushes
/// (`ThumbService::request`'s own retry, `set_visible`, `prioritize`, a rescan's
/// `enqueue_pending`) can make it eligible early either, which is what "even" means here.
/// Without this, a suspect popped again the moment it's re-queued retries within about a
/// millisecond, waits out another [`SUSPECT_WAIT`] as a writer - blocking every new `read()`
/// behind it for that whole second, per parking_lot's task-fairness - times out again, and
/// repeats for as long as the stuck decode lasts: busy work with nothing to show for it, and a
/// warning logged roughly twice a second. Backing off costs the suspect nothing but time - it
/// holds no lock and occupies no worker while it waits (`ThumbQueue::defer`) - and doubling
/// means a decode stuck for a long time is retried less and less often rather than at a fixed
/// rate forever.
const SUSPECT_BACKOFF_START: Duration = Duration::from_secs(2);

/// The most a suspect ever waits between retries, however many times it has already backed
/// off. Unbounded doubling would eventually make a suspect wait longer than most stuck decodes
/// plausibly last, which would cost real staleness for no benefit once the exponent gets large.
const SUSPECT_BACKOFF_MAX: Duration = Duration::from_secs(30);

/// Worker threads for thumbnail generation: all cores but one, so the UI stays responsive,
/// and never more than [`MAX_WORKERS`].
pub fn default_workers() -> usize {
    worker_count(std::thread::available_parallelism().map_or(1, |n| n.get()))
}

fn worker_count(parallelism: usize) -> usize {
    parallelism.saturating_sub(1).clamp(1, MAX_WORKERS)
}

pub struct ThumbService {
    lib: Arc<Library>,
    cache: Arc<ThumbCache>,
    queue: Arc<ThumbQueue>,
    workers: Vec<JoinHandle<()>>,
    render: RenderFn,
    inflight: Arc<InFlight>,
    /// `decode_lock` and its backoff bookkeeping, grouped into one field so `process` takes
    /// one parameter for both rather than two (and stays under clippy's argument-count lint).
    suspects: Arc<Suspects>,
}

#[derive(Default)]
struct Suspects {
    /// Excludes a suspect's decode from every other decode, so a group of photos queued
    /// together does not inherit one photo's deaths merely for having been in flight beside
    /// it when it died. A photo with at least one recorded death takes this exclusively
    /// (`write`); everything else only needs to keep other *exclusive* holders out, not each
    /// other, so it takes `read`. See `process`.
    lock: RwLock<()>,
    /// How long each suspect currently waits before its next retry - see `SUSPECT_BACKOFF_START`.
    backoff: SuspectBackoff,
}

/// Per-id backoff for a suspect that has just failed to get `decode_lock` in time. Doubles
/// from [`SUSPECT_BACKOFF_START`] on each further timeout, capped at [`SUSPECT_BACKOFF_MAX`],
/// and forgotten once the id actually gets to decode - a later, unrelated suspect run (a fresh
/// death recorded after an earlier retry succeeded) starts from the short wait again rather
/// than wherever a previous run's timeouts left off.
#[derive(Default)]
struct SuspectBackoff {
    steps: parking_lot::Mutex<std::collections::HashMap<i64, u32>>,
    /// Every timeout across every id, scoped to one `SuspectBackoff` (so one service's own
    /// tests aren't disturbed by another running concurrently). Write-only outside tests:
    /// production only ever increments it, from `bump`, and nothing there reads it back - a
    /// test loads it to count how many real `decode_lock` attempts actually happened.
    attempts: std::sync::atomic::AtomicUsize,
}

impl SuspectBackoff {
    /// Records another timeout for `id` and returns how long it should wait before its next
    /// attempt.
    fn bump(&self, id: i64) -> Duration {
        self.attempts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut steps = self.steps.lock();
        let step = steps.entry(id).or_insert(0);
        let wait = SUSPECT_BACKOFF_START
            .checked_mul(1 << (*step).min(8))
            .unwrap_or(SUSPECT_BACKOFF_MAX)
            .min(SUSPECT_BACKOFF_MAX);
        *step += 1;
        wait
    }

    /// Forgets `id`'s backoff once it actually holds `decode_lock` - it is no longer waiting
    /// on anything, so its next timeout (if there is one, some other day) starts fresh.
    fn reset(&self, id: i64) {
        self.steps.lock().remove(&id);
    }

    /// Whether `id` currently has a recorded backoff step - it has timed out on
    /// `decode_lock` at least once since its last reset. `deaths`, which decides whether a
    /// photo takes the suspect path at all, is keyed by the item's *current* `thumb_key()`,
    /// but a step count is keyed by id alone - so an id whose file changed or was edited
    /// can stop being a suspect (a fresh key has no death record) while still carrying a
    /// step count from before the change. Used by the ordinary decode path to notice that
    /// and clear it - see `process`.
    fn has_steps(&self, id: i64) -> bool {
        self.steps.lock().contains_key(&id)
    }
}

impl Suspects {
    /// Clears every trace of `id`'s backoff - its step count (`SuspectBackoff::reset`) and
    /// any pending `ThumbQueue::delayed` entry (`ThumbQueue::forget`) - once its fate no
    /// longer depends on winning `decode_lock` again: it has decoded, it has been found
    /// gone or already failed, it was failed outright (`DEATHS_TO_FAIL`), or it turned out
    /// not to be a suspect any more (`SuspectBackoff::has_steps`, called from the ordinary
    /// decode path). The queue half is a no-op for every one of those in the real queue
    /// path - `admit_due` already took `id` out of `delayed` before `pop` handed the job to
    /// a worker - so it only ever has something to remove for an id resolved through
    /// `get_or_generate`, which bypasses the queue entirely (test-only today, see its own
    /// doc); kept anyway since `ThumbQueue::forget` is cheap and a no-op is exactly as safe
    /// as not calling it.
    fn resolved(&self, id: i64, queue: &ThumbQueue) {
        self.backoff.reset(id);
        queue.forget(id);
    }
}

/// Decodes a source file into (preview, grid) images. A seam so tests can inject a
/// misbehaving decoder; production always uses [`ThumbCache::render`].
type RenderFn = fn(&ThumbCache, &Path, u8, Edit) -> Result<(DynamicImage, DynamicImage)>;

fn default_render(
    cache: &ThumbCache,
    source: &Path,
    orientation: u8,
    edit: Edit,
) -> Result<(DynamicImage, DynamicImage)> {
    cache.render(source, orientation, edit)
}

/// Guarantees `queue.done(id)` runs exactly once per popped job, even if the job panics.
struct DoneGuard<'a>(&'a ThumbQueue, i64);

impl Drop for DoneGuard<'_> {
    fn drop(&mut self) {
        self.0.done(self.1);
    }
}

impl ThumbService {
    pub fn start(lib: Arc<Library>, cache: Arc<ThumbCache>, workers: usize) -> Self {
        Self::start_with(lib, cache, workers, default_render)
    }

    fn start_with(
        lib: Arc<Library>,
        cache: Arc<ThumbCache>,
        workers: usize,
        render: RenderFn,
    ) -> Self {
        let queue = Arc::new(ThumbQueue::new());
        let inflight = Arc::new(InFlight::new(cache.root()));
        // Before any worker starts: a marker still here was in flight when a previous run died.
        inflight.recover();
        let suspects = Arc::new(Suspects::default());
        let workers = (0..workers.max(1))
            .map(|i| {
                let (lib, cache, queue, inflight, suspects) = (
                    lib.clone(),
                    cache.clone(),
                    queue.clone(),
                    inflight.clone(),
                    suspects.clone(),
                );
                std::thread::Builder::new()
                    .name(format!("photon-thumb-{i}"))
                    .spawn(move || {
                        while let Some(id) = queue.pop_blocking() {
                            let _guard = DoneGuard(&queue, id);
                            if let Err(err) =
                                process(&lib, &cache, &inflight, &suspects, &queue, id, render)
                            {
                                tracing::warn!(id, %err, "thumbnail job failed");
                            }
                        }
                    })
                    .expect("failed to spawn thumbnail worker")
            })
            .collect();
        Self {
            lib,
            cache,
            queue,
            workers,
            render,
            inflight,
            suspects,
        }
    }

    /// Queues every item still waiting for thumbnails at background priority.
    pub fn enqueue_pending(&self) -> Result<usize> {
        let ids = self.lib.pending_thumb_ids()?;
        self.queue.push_many(&ids, Priority::Background);
        Ok(ids.len())
    }

    pub fn set_visible(&self, ids: &[i64]) {
        self.queue.set_visible(ids);
    }

    pub fn prioritize(&self, ids: &[i64], priority: Priority) {
        self.queue.push_many(ids, priority);
    }

    /// Disarms the crash-loop guard: a deliberate quit is not the kind of death it watches
    /// for, so nothing from here on should be left for the next launch to misread as one.
    /// Idempotent - `close` calls it again itself, right before the queue stops taking jobs;
    /// a caller that needs the guard disarmed *earlier* than that (`Engine::shutdown` does,
    /// because it does other things first that can themselves stall) calls this directly.
    pub fn disarm(&self) {
        self.inflight.disarm();
    }

    /// Closes the queue so every worker finishes its current job and then stops.
    /// Does not join the workers itself; `Drop` still does that.
    ///
    /// Disarms the crash-loop guard first: a deliberate close is not the kind of death it
    /// watches for, so no marker from it should survive to be misread as one. Safe to call
    /// even if `disarm` already ran - it just sweeps an already-empty directory again.
    pub fn close(&self) {
        self.inflight.disarm();
        self.queue.close();
    }

    /// Returns the cached thumbnail, generating it on the calling thread if needed.
    ///
    /// Not the production path - `protocol.rs` uses `request`, which keeps decoding inside
    /// the worker pool - and today only the tests call this, as a synchronous harness around
    /// `process`. Kept public rather than gated to tests because `self.render` would
    /// otherwise be a field no non-test code reads; if a second caller ever appears, it
    /// should be `request` unless it genuinely wants to decode on its own thread.
    pub fn get_or_generate(&self, id: i64, size: ThumbSize) -> Result<PathBuf> {
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        if item.thumb_state == ThumbState::Failed {
            return Err(Error::ThumbFailed(item.thumb_error.unwrap_or_default()));
        }
        let path = self.cache.path_for(item.thumb_key(), size);
        if path.is_file() {
            return Ok(path);
        }
        process(
            &self.lib,
            &self.cache,
            &self.inflight,
            &self.suspects,
            &self.queue,
            id,
            self.render,
        )?;
        if path.is_file() {
            return Ok(path);
        }
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        Err(Error::ThumbFailed(
            item.thumb_error
                .unwrap_or_else(|| "thumbnail unavailable".into()),
        ))
    }

    /// Returns the cached thumbnail, or moves the item to the front of the queue and waits
    /// for a worker to build it. Concurrent requests for one item share the same decode,
    /// and CPU use stays within the worker pool. Used by the `photon://` protocol.
    pub fn request(&self, id: i64, size: ThumbSize, timeout: Duration) -> Result<PathBuf> {
        let deadline = Instant::now() + timeout;
        // Two rounds: the first may only wait out a job already running for an older
        // version of the file. A suspect currently backing off (`ThumbQueue::delayed`)
        // costs this loop nothing extra: `push` leaves it exactly where it is rather than
        // readmitting it (see `SUSPECT_BACKOFF_START`), so `wait_for` finds it neither
        // queued nor in flight and returns at once - no worker is ever woken for it, so
        // this never causes a fresh `decode_lock` attempt on the suspect's behalf.
        for _ in 0..2 {
            if let Some(path) = self.cached(id, size)? {
                return Ok(path);
            }
            self.queue.push(id, Priority::Visible);
            if !self.queue.wait_for(id, deadline) {
                return Err(Error::ThumbTimeout(id));
            }
        }
        self.cached(id, size)?.ok_or(Error::ThumbUnavailable(id))
    }

    /// The cached thumbnail for `id` at `size`, or `None` if it has not been built yet.
    ///
    /// The checks `request` makes before and after waiting, in one place rather than two
    /// copies of four lines: `missing_since` was added to the copy inside the loop only, so
    /// an item that went missing *while* its thumbnail was being built reported
    /// `ThumbUnavailable` rather than `NotFound` - a 503 the tile would keep retrying
    /// instead of a 404 it can give up on.
    fn cached(&self, id: i64, size: ThumbSize) -> Result<Option<PathBuf>> {
        let item = self.lib.item(id)?.ok_or(Error::NotFound(id))?;
        if item.missing_since.is_some() {
            return Err(Error::NotFound(id));
        }
        if item.thumb_state == ThumbState::Failed {
            return Err(Error::ThumbFailed(item.thumb_error.unwrap_or_default()));
        }
        let path = self.cache.path_for(item.thumb_key(), size);
        Ok(path.is_file().then_some(path))
    }

    /// Where the thumbnail of the picture `key` names is cached at `size`, whether or not it
    /// has been built. A key names one picture - the file's fingerprint and its edit - so
    /// what is cached there is that picture whichever item asks.
    pub fn path_for(&self, key: u64, size: ThumbSize) -> PathBuf {
        self.cache.path_for(key, size)
    }

    pub fn wait_idle(&self) {
        self.queue.wait_idle();
    }

    /// Thumbnails under no live key, and the crash guard's death records under one - the
    /// same `live` set answers both, and both are left behind by the same writes.
    pub fn collect_garbage(&self) -> Result<usize> {
        let live = self.lib.live_fingerprints()?;
        Ok(self.cache.collect_garbage(&live)? + self.inflight.collect_garbage(&live))
    }
}

impl Drop for ThumbService {
    fn drop(&mut self) {
        self.queue.close();
        for handle in self.workers.drain(..) {
            let _ = handle.join();
        }
    }
}

const PANIC_MESSAGE: &str = "decoder panicked";

/// Generates thumbnails for one item.
///
/// A decode failure means the source file itself is bad: it's recorded as `Failed` on
/// the item, not returned, so the worker doesn't retry it. Anything else (the source
/// can't be opened because its drive is unplugged or it's being replaced, or the cache
/// is unwritable) is propagated as `Err` and the item is left `Pending` for a later retry.
///
/// A panicking decoder is contained: the item is recorded as `Failed` ("decoder
/// panicked") and `Err(ThumbFailed)` is returned, so neither a worker thread nor an
/// on-demand caller goes down with it.
///
/// State writes go through `set_thumb_state_if_unchanged` so a rescan that replaces this
/// item mid-decode (resetting it to `Pending`) can't be clobbered by a stale result.
///
/// `catch_unwind` above only contains a panic that actually unwinds. A panic inside rav1d's
/// `extern "C"` entry points cannot unwind and aborts, as can an allocation failure or the
/// OOM killer; none of those run `Drop`, so `inflight`'s marker/record pair is what survives
/// them: `deaths(id, key)` is read before the decode, keyed by the item's *current*
/// `thumb_key()` so a reused id, a changed file or a fresh edit never inherits another
/// photo's deaths. At [`DEATHS_TO_FAIL`] or more the photo is failed with [`CRASH_MESSAGE`]
/// without calling `render` again, rather than dying the same way on every launch. Below
/// that, `begin` holds a marker across the decode so a death leaves one behind for the next
/// launch's `recover` to count, and turn into a record - `inflight` never re-counts a
/// leftover marker itself, which is what let a single death compound into a false failure in
/// this guard's first version.
///
/// A photo with at least one recorded death also takes `decode_lock` exclusively (`write`)
/// across its render, where every other decode only takes `read`: without that, a batch of
/// photos queued together dies as a group, and every one of them - not just the culprit -
/// looks like a repeat suspect at the next launch, since they were all genuinely in flight
/// together. `decode_lock` is acquired *before* `begin` is even called, not after: a photo
/// merely waiting for a suspect's turn must never have a marker on disk for it - only the one
/// actually decoding may. `marker.resolve(decided)` then removes it again before this
/// function returns, so the lock is still held while that happens; only after does
/// `decode_guard` itself drop and let the next waiter in.
///
/// `resolve`'s `decided` is `true` once this attempt settled the photo's fate one way or
/// another (rendered, or a caught panic explicitly failed it) and `false` for a transient
/// error that leaves the item `Pending` - the marker is always removed either way (the decode
/// finished, whatever it decided), but the death *record* only when `decided`, so a suspect
/// whose drive merely dropped out for a moment does not lose the death that made it a suspect.
fn process(
    lib: &Library,
    cache: &ThumbCache,
    inflight: &InFlight,
    suspects: &Suspects,
    queue: &ThumbQueue,
    id: i64,
    render: RenderFn,
) -> Result<()> {
    let Some(item) = lib.item(id)? else {
        // Purged: gone, so any backoff held against it is moot - see `Suspects::resolved`.
        suspects.resolved(id, queue);
        return Ok(());
    };
    if item.missing_since.is_some() || item.thumb_state == ThumbState::Failed {
        // Already decided by some other means (a rescan) since whatever queued this job -
        // see `Suspects::resolved`.
        suspects.resolved(id, queue);
        return Ok(());
    }
    let key = item.thumb_key();
    // No decoder will run for an already-cached photo, so there is nothing to mark in
    // flight and nothing to guard: skip the marker and the lock entirely, rather than paying
    // `begin`'s four filesystem operations for a decode that was never going to happen. The
    // photo's fate is decided either way - `Ready` - so any death record still standing
    // against it (photon died with it in flight once, then this attempt finds the thumbnail
    // was already there - a scan re-touching an unchanged file, say) is resolved the same
    // way a successful `Marker::resolve(true)` would have cleared it.
    if cache.is_complete(key) {
        if item.thumb_state != ThumbState::Ready {
            lib.set_thumb_state_if_unchanged(&item, ThumbState::Ready, None)?;
        }
        inflight.clear(id);
        suspects.resolved(id, queue);
        return Ok(());
    }
    let deaths = inflight.deaths(id, key);
    if deaths >= DEATHS_TO_FAIL {
        tracing::error!(
            id,
            path = %item.path,
            deaths,
            "photon died with this photo in flight; not decoding it again"
        );
        lib.set_thumb_state_if_unchanged(&item, ThumbState::Failed, Some(CRASH_MESSAGE))?;
        // Clears the record even though `set_thumb_state_if_unchanged` may have left the
        // item alone because it changed underneath (a rescan mid-call): `deaths` already
        // refuses a record whose key doesn't match the item now at `id`, so the record could
        // never have blamed whatever is there after a change. This is just hygiene.
        inflight.clear(id);
        suspects.resolved(id, queue);
        return Ok(());
    }
    // Acquired before `begin`, not after: a photo blocked here - waiting its turn behind a
    // suspect's exclusive decode - must never have a marker on disk for a decode that has not
    // actually started.
    let _decode_guard = if deaths >= 1 {
        match suspects.lock.try_write_for(SUSPECT_WAIT) {
            Some(guard) => {
                // No longer waiting on anything - a later timeout, if there ever is one,
                // starts from the short wait again rather than wherever this run left off.
                // Also drops any delayed entry this id might somehow still have (it
                // shouldn't, having just been admitted to get here, but see
                // `Suspects::resolved`).
                suspects.resolved(id, queue);
                DecodeGuard::Suspect(guard)
            }
            None => {
                // parking_lot's `RwLock` is task-fair: once a writer is waiting, every
                // later `read()` queues behind it too, even one for an unrelated photo. A
                // suspect stuck behind one slow ordinary decode (an unplugged network
                // share, say) would otherwise stall the whole pool - every later ordinary
                // decode waits on a write attempt that isn't even the one blocking it.
                // Backing off after a bounded wait costs this one suspect a retry; nothing
                // is decided (no marker was ever written, the death record stands), so the
                // item stays `Pending`. It goes back on the queue *deferred*, not merely
                // re-pushed: a plain `push` would be popped again within about a
                // millisecond with the queue otherwise idle, retry the same doomed write,
                // time out again, and repeat for as long as the stuck decode lasts - the
                // busy-loop-with-a-log-line `SUSPECT_BACKOFF_START` exists to stop. The wait
                // doubles on each further timeout for the same id, so it is retried less and
                // less often rather than at a fixed rate for as long as the block lasts, and
                // is forgotten (`backoff.reset`) the moment a retry actually succeeds.
                let wait = suspects.backoff.bump(id);
                tracing::warn!(
                    id,
                    path = %item.path,
                    ?wait,
                    "suspect decode could not get exclusive access in time; backing off"
                );
                queue.defer(id, Priority::Background, Instant::now() + wait);
                return Err(Error::ThumbUnavailable(id));
            }
        }
    } else {
        // The file changed, or the photo was edited: `deaths` above is keyed by the item's
        // *current* `thumb_key()`, so a fresh key with no death record takes this ordinary
        // path even for an id that used to be a suspect - but `backoff`'s step count is
        // keyed by id alone, so it wouldn't otherwise notice. Clear it (and any delayed
        // entry, though `pop` already emptied that for this job - see `Suspects::resolved`)
        // so it doesn't outlive the death record that justified it.
        if suspects.backoff.has_steps(id) {
            suspects.resolved(id, queue);
        }
        DecodeGuard::Ordinary(suspects.lock.read())
    };
    // Held across the decode, so it is on disk if the decode takes the process down.
    let marker = inflight.begin(id, key);
    let (result, decided) =
        match catch_unwind(AssertUnwindSafe(|| process_item(lib, cache, &item, render))) {
            Ok(Ok(())) => (Ok(()), true),
            Ok(Err(err)) => (Err(err), false),
            Err(_) => {
                tracing::error!(id, path = %item.path, "thumbnail decoder panicked");
                lib.set_thumb_state_if_unchanged(&item, ThumbState::Failed, Some(PANIC_MESSAGE))?;
                (Err(Error::ThumbFailed(PANIC_MESSAGE.into())), true)
            }
        };
    // Resolved (and so dropped) here, still inside `decode_guard`'s scope: the marker and, if
    // decided, the record are both gone from disk before the lock lets the next photo through.
    marker.resolve(decided);
    result
}

/// Held across one decode: a suspect (at least one recorded death) takes the lock
/// exclusively so no other decode runs beside it, and an ordinary photo only takes a shared
/// read, which blocks a suspect's `write` but not another ordinary decode. Its only job is to
/// stay alive until the decode finishes; nothing reads through it.
#[allow(
    dead_code,
    reason = "held only for its Drop; nothing ever reads through it"
)]
enum DecodeGuard<'a> {
    Suspect(RwLockWriteGuard<'a, ()>),
    Ordinary(RwLockReadGuard<'a, ()>),
}

fn process_item(lib: &Library, cache: &ThumbCache, item: &Item, render: RenderFn) -> Result<()> {
    let fp = item.thumb_key();
    if !cache.is_complete(fp) {
        match render(cache, Path::new(&item.path), item.orientation, item.edit) {
            Ok((preview, grid)) => cache.store(fp, &preview, &grid)?,
            Err(err) if !is_source_defect(&err) => return Err(err),
            Err(err) => {
                lib.set_thumb_state_if_unchanged(item, ThumbState::Failed, Some(&err.to_string()))?;
                return Ok(());
            }
        }
    }
    if item.thumb_state != ThumbState::Ready {
        lib.set_thumb_state_if_unchanged(item, ThumbState::Ready, None)?;
    }
    Ok(())
}

/// Whether a render error means the source file itself is bad (so the item is `Failed`).
/// I/O errors, including those surfaced by the decoder, mean the file couldn't be read
/// right now (offline drive, permissions, mid-replace) and must not be recorded.
fn is_source_defect(err: &Error) -> bool {
    matches!(err, Error::Image(e) if !matches!(e, image::ImageError::IoError(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::NewItem;
    use crate::testutil::{jpeg_bytes, new_item, seed_folder, write_file};
    use tempfile::TempDir;

    /// Pure logic, no real time needed: `bump` doubles the wait from `SUSPECT_BACKOFF_START`
    /// on each call for the same id, stops climbing once it hits `SUSPECT_BACKOFF_MAX`,
    /// `reset` starts that id's schedule over from the top, and a different id's schedule is
    /// independent of it. Probe: change `*step += 1` to a no-op (so every call returns the
    /// same wait) or drop the `.min(SUSPECT_BACKOFF_MAX)` cap, and this goes RED on the
    /// doubling or the cap assertion respectively; drop the `steps.lock().remove` in `reset`
    /// and it goes RED on the post-reset assertion instead.
    #[test]
    fn suspect_backoff_doubles_then_caps_then_resets_per_id() {
        let backoff = SuspectBackoff::default();

        assert_eq!(
            backoff.bump(1),
            SUSPECT_BACKOFF_START,
            "first timeout: the short wait"
        );
        assert_eq!(
            backoff.bump(1),
            SUSPECT_BACKOFF_START * 2,
            "second: doubled"
        );
        assert_eq!(
            backoff.bump(1),
            SUSPECT_BACKOFF_START * 4,
            "third: doubled again"
        );

        // Enough further timeouts to run well past the cap.
        for _ in 0..10 {
            backoff.bump(1);
        }
        assert_eq!(
            backoff.bump(1),
            SUSPECT_BACKOFF_MAX,
            "doubling stops climbing once it reaches the cap"
        );

        backoff.reset(1);
        assert_eq!(
            backoff.bump(1),
            SUSPECT_BACKOFF_START,
            "reset starts id 1's schedule over from the top"
        );

        assert_eq!(
            backoff.bump(2),
            SUSPECT_BACKOFF_START,
            "a different id's schedule was never touched by id 1's climb"
        );
    }

    fn setup(files: &[(&str, Vec<u8>)]) -> (TempDir, Arc<Library>, Arc<ThumbCache>, Vec<i64>) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Arc::new(Library::open(&dir.path().join("library.db")).unwrap());
        let photos = dir.path().join("photos");
        let (_, folder) = seed_folder(&lib, &photos);
        let items: Vec<NewItem> = files
            .iter()
            .map(|(name, bytes)| {
                let path = write_file(&photos, name, bytes);
                new_item(folder, path.to_str().unwrap(), 0)
            })
            .collect();
        let ids = lib.insert_items(&items).unwrap();
        let cache = Arc::new(ThumbCache::new(dir.path().join("cache")));
        (dir, lib, cache, ids)
    }

    fn state(lib: &Library, id: i64) -> ThumbState {
        lib.item(id).unwrap().unwrap().thumb_state
    }

    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    static COUNTED_RENDERS: AtomicUsize = AtomicUsize::new(0);

    fn counting_render(
        cache: &ThumbCache,
        source: &Path,
        orientation: u8,
        edit: Edit,
    ) -> Result<(DynamicImage, DynamicImage)> {
        COUNTED_RENDERS.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(100));
        cache.render(source, orientation, edit)
    }

    fn slow_render(
        cache: &ThumbCache,
        source: &Path,
        orientation: u8,
        edit: Edit,
    ) -> Result<(DynamicImage, DynamicImage)> {
        std::thread::sleep(Duration::from_millis(500));
        cache.render(source, orientation, edit)
    }

    #[test]
    fn concurrent_requests_share_one_decode() {
        let (_dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(64, 32))]);
        let service = Arc::new(ThumbService::start_with(lib, cache, 2, counting_render));
        let id = ids[0];
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let service = service.clone();
                std::thread::spawn(move || {
                    service.request(id, ThumbSize::Grid, Duration::from_secs(10))
                })
            })
            .collect();
        for h in handles {
            assert!(h.join().unwrap().unwrap().is_file());
        }
        assert_eq!(COUNTED_RENDERS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn request_times_out_but_keeps_the_job() {
        let (_dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(64, 32))]);
        let service = ThumbService::start_with(lib.clone(), cache, 1, slow_render);
        assert!(matches!(
            service.request(ids[0], ThumbSize::Grid, Duration::from_millis(50)),
            Err(Error::ThumbTimeout(_))
        ));
        service.wait_idle();
        assert_eq!(state(&lib, ids[0]), ThumbState::Ready);
    }

    #[test]
    fn request_reports_failed_unknown_and_unreadable_items() {
        let (dir, lib, cache, ids) = setup(&[
            ("bad.jpg", b"garbage".to_vec()),
            ("gone.jpg", jpeg_bytes(8, 8)),
        ]);
        std::fs::remove_file(dir.path().join("photos").join("gone.jpg")).unwrap();
        let service = ThumbService::start(lib, cache, 1);
        let t = Duration::from_secs(10);
        assert!(matches!(
            service.request(ids[0], ThumbSize::Grid, t),
            Err(Error::ThumbFailed(_))
        ));
        assert!(matches!(
            service.request(ids[1], ThumbSize::Grid, t),
            Err(Error::ThumbUnavailable(_))
        ));
        assert!(matches!(
            service.request(9_999, ThumbSize::Grid, t),
            Err(Error::NotFound(9_999))
        ));
    }

    #[test]
    fn worker_count_leaves_a_core_free_and_stops_at_the_cap() {
        assert_eq!(worker_count(1), 1, "a single core still gets one worker");
        assert_eq!(worker_count(4), 3, "one core stays free for the UI");
        assert_eq!(
            worker_count(64),
            MAX_WORKERS,
            "peak decode memory is bounded by the cap, not by the core count"
        );
    }

    #[test]
    fn processes_pending_items_in_background() {
        let (_dir, lib, cache, ids) =
            setup(&[("a.jpg", jpeg_bytes(64, 32)), ("b.jpg", jpeg_bytes(32, 64))]);
        let service = ThumbService::start(lib.clone(), cache.clone(), 2);
        assert_eq!(service.enqueue_pending().unwrap(), 2);
        service.wait_idle();
        for id in ids {
            assert_eq!(state(&lib, id), ThumbState::Ready);
            assert!(cache.is_complete(lib.item(id).unwrap().unwrap().thumb_key()));
        }
        assert!(lib.pending_thumb_ids().unwrap().is_empty());
    }

    #[test]
    fn records_failures_instead_of_retrying() {
        let (_dir, lib, cache, ids) = setup(&[("bad.jpg", b"garbage".to_vec())]);
        let service = ThumbService::start(lib.clone(), cache, 1);
        service.enqueue_pending().unwrap();
        service.wait_idle();
        let item = lib.item(ids[0]).unwrap().unwrap();
        assert_eq!(item.thumb_state, ThumbState::Failed);
        assert!(item.thumb_error.is_some());
        assert_eq!(service.enqueue_pending().unwrap(), 0);
    }

    #[test]
    fn get_or_generate_builds_on_demand() {
        let (_dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(64, 32))]);
        let service = ThumbService::start(lib.clone(), cache, 1);
        let path = service.get_or_generate(ids[0], ThumbSize::Grid).unwrap();
        assert!(path.is_file());
        assert_eq!(state(&lib, ids[0]), ThumbState::Ready);
        assert_eq!(
            service
                .get_or_generate(ids[0], ThumbSize::Preview)
                .unwrap()
                .extension()
                .unwrap(),
            "webp"
        );
    }

    #[test]
    fn an_edited_photo_gets_thumbnails_of_the_edit_under_its_own_key() {
        let (_dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(64, 32))]);
        let service = ThumbService::start(lib.clone(), cache.clone(), 1);
        let plain = service.get_or_generate(ids[0], ThumbSize::Grid).unwrap();
        assert_eq!(image::image_dimensions(&plain).unwrap(), (64, 32));

        lib.set_item_edit(ids[0], Edit::new(1, None).unwrap())
            .unwrap();
        assert_eq!(state(&lib, ids[0]), ThumbState::Pending);
        let turned = service.get_or_generate(ids[0], ThumbSize::Grid).unwrap();
        assert_ne!(turned, plain, "a different key, so a different file");
        assert_eq!(image::image_dimensions(&turned).unwrap(), (32, 64));
        assert_eq!(state(&lib, ids[0]), ThumbState::Ready);
    }

    #[test]
    fn get_or_generate_reports_failures_and_unknown_items() {
        let (_dir, lib, cache, ids) = setup(&[("bad.jpg", b"garbage".to_vec())]);
        let service = ThumbService::start(lib, cache, 1);
        assert!(matches!(
            service.get_or_generate(ids[0], ThumbSize::Grid),
            Err(Error::ThumbFailed(_))
        ));
        assert!(matches!(
            service.get_or_generate(ids[0], ThumbSize::Grid),
            Err(Error::ThumbFailed(_))
        ));
        assert!(matches!(
            service.get_or_generate(9_999, ThumbSize::Grid),
            Err(Error::NotFound(9_999))
        ));
    }

    #[test]
    fn store_failure_leaves_item_pending() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Arc::new(Library::open(&dir.path().join("library.db")).unwrap());
        let photos = dir.path().join("photos");
        let (_, folder) = seed_folder(&lib, &photos);
        let path = write_file(&photos, "a.jpg", &jpeg_bytes(64, 32));
        let ids = lib
            .insert_items(&[new_item(folder, path.to_str().unwrap(), 0)])
            .unwrap();
        // The cache root is a regular file, so `create_dir_all` inside `store` fails: a
        // stand-in for a full disk or a permissions problem, distinct from a bad source file.
        let cache_root = dir.path().join("cache");
        std::fs::write(&cache_root, b"not a directory").unwrap();
        let cache = Arc::new(ThumbCache::new(cache_root));
        let service = ThumbService::start(lib.clone(), cache, 1);

        assert!(service.get_or_generate(ids[0], ThumbSize::Grid).is_err());
        assert_eq!(state(&lib, ids[0]), ThumbState::Pending);
    }

    #[test]
    fn missing_source_file_leaves_item_pending() {
        let (dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(64, 32))]);
        // Drive unplugged / file being replaced: the source can't be opened at all.
        std::fs::remove_file(dir.path().join("photos").join("a.jpg")).unwrap();
        let service = ThumbService::start(lib.clone(), cache, 1);

        assert!(matches!(
            service.get_or_generate(ids[0], ThumbSize::Grid),
            Err(Error::Io(_))
        ));
        assert_eq!(state(&lib, ids[0]), ThumbState::Pending);

        service.enqueue_pending().unwrap();
        service.wait_idle();
        assert_eq!(state(&lib, ids[0]), ThumbState::Pending);
        assert_eq!(lib.pending_thumb_ids().unwrap(), ids);
    }

    #[test]
    fn only_decode_errors_mark_failed() {
        let io = || std::io::Error::other("unplugged");
        assert!(is_source_defect(&Error::Image(
            image::ImageError::Unsupported(image::error::UnsupportedError::from(
                image::error::ImageFormatHint::Unknown
            ))
        )));
        assert!(!is_source_defect(&Error::Image(
            image::ImageError::IoError(io())
        )));
        assert!(!is_source_defect(&Error::Io(io())));
    }

    fn panicking_render(
        cache: &ThumbCache,
        source: &Path,
        orientation: u8,
        edit: Edit,
    ) -> Result<(DynamicImage, DynamicImage)> {
        if source.to_string_lossy().contains("panic") {
            panic!("simulated decoder bug");
        }
        cache.render(source, orientation, edit)
    }

    #[test]
    fn panicking_job_is_failed_and_worker_survives() {
        let (_dir, lib, cache, ids) = setup(&[
            ("a_panic.jpg", jpeg_bytes(64, 32)),
            ("b_ok.jpg", jpeg_bytes(64, 32)),
        ]);
        let service = ThumbService::start_with(lib.clone(), cache, 1, panicking_render);
        // Grid order puts a_panic.jpg first, so the single worker hits the panic first.
        service.enqueue_pending().unwrap();
        service.wait_idle();

        let bad = lib.item(ids[0]).unwrap().unwrap();
        assert_eq!(bad.thumb_state, ThumbState::Failed);
        assert_eq!(bad.thumb_error.as_deref(), Some("decoder panicked"));
        assert_eq!(state(&lib, ids[1]), ThumbState::Ready);
    }

    #[test]
    fn get_or_generate_contains_decoder_panics() {
        let (_dir, lib, cache, ids) = setup(&[("panic.jpg", jpeg_bytes(64, 32))]);
        let service = ThumbService::start_with(lib.clone(), cache, 1, panicking_render);
        assert!(matches!(
            service.get_or_generate(ids[0], ThumbSize::Grid),
            Err(Error::ThumbFailed(_))
        ));
        assert_eq!(state(&lib, ids[0]), ThumbState::Failed);
    }

    fn marker(dir: &TempDir, id: i64) -> std::path::PathBuf {
        dir.path()
            .join("cache")
            .join("in-flight")
            .join(id.to_string())
    }

    fn death_record(dir: &TempDir, id: i64) -> std::path::PathBuf {
        dir.path().join("cache").join("deaths").join(id.to_string())
    }

    fn item_key(lib: &Library, id: i64) -> u64 {
        lib.item(id).unwrap().unwrap().thumb_key()
    }

    /// Fails loudly if the service calls it: a photo marked failed by the guard must not be
    /// decoded again. A panic here is caught and recorded as "decoder panicked", which
    /// the tests below tell apart from the guard's own message.
    fn must_not_render(
        _: &ThumbCache,
        _: &Path,
        _: u8,
        _: Edit,
    ) -> Result<(DynamicImage, DynamicImage)> {
        panic!("the guard should have stopped this decode");
    }

    /// Renders only if a marker is on disk while it runs, which is the guard's whole point:
    /// the marker has to exist *during* the decode that might kill the process.
    fn render_requiring_a_marker(
        cache: &ThumbCache,
        source: &Path,
        orientation: u8,
        edit: Edit,
    ) -> Result<(DynamicImage, DynamicImage)> {
        let marked = std::fs::read_dir(cache.root().join("in-flight"))
            .is_ok_and(|mut entries| entries.next().is_some());
        assert!(marked, "no in-flight marker while decoding");
        default_render(cache, source, orientation, edit)
    }

    #[test]
    fn a_marker_is_on_disk_while_rendering_and_gone_after() {
        let (dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(40, 20))]);
        let service = ThumbService::start_with(lib.clone(), cache, 1, render_requiring_a_marker);
        service.get_or_generate(ids[0], ThumbSize::Grid).unwrap();
        assert_eq!(state(&lib, ids[0]), ThumbState::Ready);
        assert!(!marker(&dir, ids[0]).exists());
        assert!(!death_record(&dir, ids[0]).exists());
    }

    #[test]
    fn a_caught_panic_leaves_no_marker() {
        // `panicking_render` only panics for a path containing "panic".
        let (dir, lib, cache, ids) = setup(&[("a_panic.jpg", jpeg_bytes(40, 20))]);
        let service = ThumbService::start_with(lib.clone(), cache, 1, panicking_render);
        assert!(service.get_or_generate(ids[0], ThumbSize::Grid).is_err());
        assert!(!marker(&dir, ids[0]).exists());
        assert!(!death_record(&dir, ids[0]).exists());
    }

    /// Photon died once with this photo in flight (one marker, recovered once): that could be
    /// the user quitting, so the photo is decoded again, and succeeding clears the record.
    #[test]
    fn one_death_is_forgiven() {
        let (dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(40, 20))]);
        let key = item_key(&lib, ids[0]);
        let inflight = InFlight::new(cache.root());
        std::mem::forget(inflight.begin(ids[0], key));
        inflight.recover();
        // Dropped before the service starts its own `InFlight`: it holds the cache root's
        // lock file for as long as it lives, and only one `InFlight` at a time can hold it
        // (see `InFlight::guarded`) - two live at once here would make the service's own
        // instance run unguarded, the same as a real second photon process would. No marker
        // is left for the service's own `recover()` to find either way, so the one death
        // already on record stands alone.
        drop(inflight);
        let service = ThumbService::start_with(lib.clone(), cache, 1, default_render);
        service.get_or_generate(ids[0], ThumbSize::Grid).unwrap();
        assert_eq!(state(&lib, ids[0]), ThumbState::Ready);
        assert!(!marker(&dir, ids[0]).exists());
        assert!(!death_record(&dir, ids[0]).exists());
    }

    /// A purged photo's death record goes with its thumbnails, in the same collection.
    #[test]
    fn garbage_collection_removes_a_purged_photos_death_record() {
        let (dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(40, 20))]);
        let key = item_key(&lib, ids[0]);
        let inflight = InFlight::new(cache.root());
        std::mem::forget(inflight.begin(ids[0], key));
        inflight.recover();
        // Dropped before the service starts, as in `one_death_is_forgiven`.
        drop(inflight);
        lib.purge_items(&ids).unwrap();

        let service = ThumbService::start_with(lib, cache, 1, must_not_render);
        assert!(death_record(&dir, ids[0]).exists());
        service.collect_garbage().unwrap();
        assert!(!death_record(&dir, ids[0]).exists());
    }

    /// Photon died a second time with this photo in flight (its death record already held
    /// one, matching, death): the photo is failed with the guard's message, the decoder is
    /// not called, and the record is cleared.
    #[test]
    fn two_deaths_fail_the_photo_without_decoding_it() {
        let (dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(40, 20))]);
        let key = item_key(&lib, ids[0]);
        let inflight = InFlight::new(cache.root());
        std::mem::forget(inflight.begin(ids[0], key));
        inflight.recover();
        std::mem::forget(inflight.begin(ids[0], key));
        // Dropped so the service's own `InFlight` can acquire the cache root's lock file
        // itself (see `one_death_is_forgiven`'s comment on the same line) - its `recover()`
        // is what turns this second marker into the second death against the same key.
        drop(inflight);
        let service = ThumbService::start_with(lib.clone(), cache, 1, must_not_render);
        assert!(service.get_or_generate(ids[0], ThumbSize::Grid).is_err());
        let item = lib.item(ids[0]).unwrap().unwrap();
        assert_eq!(item.thumb_state, ThumbState::Failed);
        assert_eq!(item.thumb_error.as_deref(), Some(CRASH_MESSAGE));
        assert!(!marker(&dir, ids[0]).exists());
        assert!(!death_record(&dir, ids[0]).exists());
    }

    /// A suspect's second death must not blame the photos merely queued alongside it: without
    /// `decode_lock`, a batch dies together and every survivor looks like a repeat suspect at
    /// the next launch too. This proves the exclusion directly: a render fn tracks whether the
    /// suspect and any other photo were ever mid-render at the same time, using two static
    /// flags rather than a single counter, because the claim is specifically "the suspect was
    /// alone", not "no two decodes ever overlapped" - ordinary photos are still allowed to
    /// overlap each other.
    #[test]
    fn a_suspect_decodes_alone() {
        static SUSPECT_ACTIVE: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);
        static OTHERS_ACTIVE: AtomicUsize = AtomicUsize::new(0);
        static OVERLAPPED: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);

        fn probe(
            cache: &ThumbCache,
            source: &Path,
            orientation: u8,
            edit: Edit,
        ) -> Result<(DynamicImage, DynamicImage)> {
            let is_suspect = source.to_string_lossy().contains("suspect");
            if is_suspect {
                if OTHERS_ACTIVE.load(Ordering::SeqCst) > 0 {
                    OVERLAPPED.store(true, Ordering::SeqCst);
                }
                SUSPECT_ACTIVE.store(true, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(100));
                SUSPECT_ACTIVE.store(false, Ordering::SeqCst);
            } else {
                OTHERS_ACTIVE.fetch_add(1, Ordering::SeqCst);
                if SUSPECT_ACTIVE.load(Ordering::SeqCst) {
                    OVERLAPPED.store(true, Ordering::SeqCst);
                }
                std::thread::sleep(Duration::from_millis(40));
                OTHERS_ACTIVE.fetch_sub(1, Ordering::SeqCst);
            }
            default_render(cache, source, orientation, edit)
        }

        // Named to sort first under GRID_ORDER's filename tie-break, so the suspect is
        // among the first jobs three workers pop, not queued behind the others: otherwise
        // it would only ever run once everything else had already finished, and the lock
        // would never be exercised.
        let (_dir, lib, cache, ids) = setup(&[
            ("a_suspect.jpg", jpeg_bytes(40, 20)),
            ("b.jpg", jpeg_bytes(40, 20)),
            ("c.jpg", jpeg_bytes(40, 20)),
            ("d.jpg", jpeg_bytes(40, 20)),
        ]);
        let suspect_key = item_key(&lib, ids[0]);
        let inflight = InFlight::new(cache.root());
        std::mem::forget(inflight.begin(ids[0], suspect_key));
        inflight.recover();
        // See `one_death_is_forgiven`'s comment: only one `InFlight` at a time can hold the
        // cache root's lock file.
        drop(inflight);

        let service = ThumbService::start_with(lib.clone(), cache, 3, probe);
        service.enqueue_pending().unwrap();
        service.wait_idle();

        assert!(
            !OVERLAPPED.load(Ordering::SeqCst),
            "the suspect decoded alongside another render"
        );
        assert_eq!(
            state(&lib, ids[0]),
            ThumbState::Ready,
            "one death is still forgiven"
        );
    }

    /// The reviewer's exact repro for the ordering bug: two suspects (one death each) and one
    /// ordinary photo, three workers, so all three can be popped and attempt to decode at
    /// once. Before the fix, `begin` wrote its marker before the decode ever held the lock,
    /// so every photo merely *blocked* on the suspect's exclusive write already had a marker
    /// on disk - the reviewer's own worktree probe found 3 markers while one suspect decoded
    /// alone, where there should be 1. A second suspect, never actually decoding, could then
    /// be falsely failed if the first's decode killed the process. Fixed order: the lock is
    /// acquired first, so a blocked photo never reaches `begin` at all until it actually
    /// holds its turn.
    #[test]
    fn only_the_decoding_suspect_holds_a_marker() {
        fn probe(
            cache: &ThumbCache,
            source: &Path,
            orientation: u8,
            edit: Edit,
        ) -> Result<(DynamicImage, DynamicImage)> {
            if source.to_string_lossy().contains("suspect") {
                // Long enough for the other two workers to reach - and, before the fix,
                // pass - their own `begin` while this one holds the lock alone.
                std::thread::sleep(Duration::from_millis(80));
                let markers = std::fs::read_dir(cache.root().join("in-flight"))
                    .map(|entries| {
                        entries
                            .flatten()
                            .filter(|e| {
                                e.path().extension().and_then(|x| x.to_str()) != Some("tmp")
                            })
                            .count()
                    })
                    .unwrap_or(0);
                assert_eq!(
                    markers, 1,
                    "more than one marker on disk while a suspect decodes alone"
                );
            }
            default_render(cache, source, orientation, edit)
        }

        let (_dir, lib, cache, ids) = setup(&[
            ("a_suspect1.jpg", jpeg_bytes(40, 20)),
            ("a_suspect2.jpg", jpeg_bytes(40, 20)),
            ("z_ordinary.jpg", jpeg_bytes(40, 20)),
        ]);
        let inflight = InFlight::new(cache.root());
        for &id in &ids[..2] {
            let key = item_key(&lib, id);
            std::mem::forget(inflight.begin(id, key));
        }
        inflight.recover();
        // See `one_death_is_forgiven`'s comment: only one `InFlight` at a time can hold the
        // cache root's lock file.
        drop(inflight);

        let service = ThumbService::start_with(lib.clone(), cache, 3, probe);
        service.enqueue_pending().unwrap();
        service.wait_idle();

        for &id in &ids[..2] {
            assert_eq!(
                state(&lib, id),
                ThumbState::Ready,
                "one death is still forgiven"
            );
        }
    }

    /// A transient failure (the source unreachable, the cache unwritable) decides nothing
    /// about the photo - the item stays `Pending` for a retry - so a suspect's death record
    /// must survive it. Losing it here would let a photo that genuinely killed photon once
    /// reset to a clean slate merely because its *next* attempt also failed before the
    /// decoder was ever called.
    #[test]
    fn a_transient_failure_keeps_the_suspects_death_record() {
        let (dir, lib, cache, ids) = setup(&[("a.jpg", jpeg_bytes(40, 20))]);
        // Drive unplugged / file being replaced: the source can't be opened at all, an
        // `Error::Io` that `process_item` propagates without deciding anything.
        std::fs::remove_file(dir.path().join("photos").join("a.jpg")).unwrap();

        let key = item_key(&lib, ids[0]);
        let inflight = InFlight::new(cache.root());
        std::mem::forget(inflight.begin(ids[0], key));
        inflight.recover();
        // See `one_death_is_forgiven`'s comment: only one `InFlight` at a time can hold the
        // cache root's lock file.
        drop(inflight);

        let service = ThumbService::start_with(lib.clone(), cache, 1, default_render);
        assert!(matches!(
            service.get_or_generate(ids[0], ThumbSize::Grid),
            Err(Error::Io(_))
        ));
        assert_eq!(
            state(&lib, ids[0]),
            ThumbState::Pending,
            "not decided, just couldn't even start"
        );
        assert!(
            !marker(&dir, ids[0]).exists(),
            "the marker itself is always cleared"
        );
        assert!(
            death_record(&dir, ids[0]).exists(),
            "but the death record survives a transient failure"
        );
    }

    #[test]
    fn collect_garbage_removes_thumbnails_of_purged_items() {
        let (_dir, lib, cache, ids) =
            setup(&[("a.jpg", jpeg_bytes(64, 32)), ("b.jpg", jpeg_bytes(64, 32))]);
        let service = ThumbService::start(lib.clone(), cache, 1);
        service.enqueue_pending().unwrap();
        service.wait_idle();
        lib.purge_items(&[ids[1]]).unwrap();
        assert_eq!(service.collect_garbage().unwrap(), 2);
        assert!(service.get_or_generate(ids[0], ThumbSize::Grid).is_ok());
    }

    /// The reviewer's exact repro: an ordinary decode stuck for 3s (an unplugged network
    /// share, say) holding `decode_lock` as a reader, a suspect queued behind it wanting the
    /// lock exclusively, and an ordinary photo queued last. Before the fix, the suspect's
    /// plain `write()` registered as a waiting writer as soon as it tried, and parking_lot's
    /// `RwLock` is task-fair: every *later* `read()` - including the unrelated ordinary
    /// photo's - then queues behind that wait too, so the ordinary photo did not even start
    /// decoding until the stuck one finished, at 3.006s. With `try_write_for(SUSPECT_WAIT)`
    /// the suspect gives up well before that, which stops registering as a waiting writer and
    /// lets the ordinary photo's `read()` through.
    ///
    /// Made deterministic rather than relying on scheduling luck: the three photos are queued
    /// one at a time, each only once the previous step has actually happened - the hang
    /// decode's render literally started (a signal set at the top of the probe, before it
    /// sleeps, not merely popped off the queue), then the suspect's write attempt is actually
    /// registered (`suspects.lock.is_locked_exclusive()`, true the moment a writer is waiting -
    /// parking_lot's `WRITER_BIT` - not only once one is granted). Only then is the ordinary
    /// photo queued, so it can never by chance be popped, or the suspect's write attempted,
    /// before the hang decode is holding the lock as a reader. The reviewer measured this
    /// construction at 5/5 RED on the code before this fix (3.003s each run) and 1.0006s on
    /// the fixed code.
    ///
    /// The two spin loops below need a bound generous enough never to false-fail under a slow
    /// or loaded CI runner, so each has its own timeout that panics loudly (with a clear
    /// message) rather than hanging the suite forever if its *own* signal never arrives - a
    /// setup step silently broken some other way, not the fix this test is actually about. A
    /// wrong fix here does not hang at all: both signals still arrive (the hang decode still
    /// starts, the suspect still registers as a waiting writer), so the spin loops pass either
    /// way, and the reverted code fails cleanly and quickly on the final timing assertion below
    /// instead (reliably 3.003s, per the reviewer's measurement above). The final assertion's
    /// bound is untouched by the spin loops' generosity: nothing in them makes the ordinary
    /// decode start any later than the fix actually allows, so it can still only ever
    /// false-*pass*, never false-*fail*.
    #[test]
    fn a_stuck_ordinary_decode_behind_a_waiting_suspect_does_not_stall_the_pool() {
        static HANG_STARTED: OnceLock<()> = OnceLock::new();
        static TEST_START: OnceLock<Instant> = OnceLock::new();
        static ORDINARY_STARTED_AFTER: OnceLock<Duration> = OnceLock::new();

        fn probe(
            cache: &ThumbCache,
            source: &Path,
            orientation: u8,
            edit: Edit,
        ) -> Result<(DynamicImage, DynamicImage)> {
            let name = source.to_string_lossy();
            if name.contains("hang") {
                let _ = HANG_STARTED.set(());
                std::thread::sleep(Duration::from_secs(3));
            } else if name.contains("ordinary") {
                let start = *TEST_START
                    .get()
                    .expect("start recorded before the service ran");
                let _ = ORDINARY_STARTED_AFTER.set(start.elapsed());
            }
            default_render(cache, source, orientation, edit)
        }

        let (_dir, lib, cache, ids) = setup(&[
            ("a_hang.jpg", jpeg_bytes(40, 20)),
            ("b_suspect.jpg", jpeg_bytes(40, 20)),
            ("c_ordinary.jpg", jpeg_bytes(40, 20)),
        ]);
        let (hang_id, suspect_id, ordinary_id) = (ids[0], ids[1], ids[2]);
        let suspect_key = item_key(&lib, suspect_id);
        let inflight = InFlight::new(cache.root());
        std::mem::forget(inflight.begin(suspect_id, suspect_key));
        inflight.recover();
        // See `one_death_is_forgiven`'s comment: only one `InFlight` at a time can hold the
        // cache root's lock file.
        drop(inflight);

        TEST_START.set(Instant::now()).unwrap();
        let service = ThumbService::start_with(lib.clone(), cache, 3, probe);

        service.prioritize(&[hang_id], Priority::Background);
        let deadline = Instant::now() + Duration::from_secs(5);
        while HANG_STARTED.get().is_none() {
            assert!(Instant::now() < deadline, "the hang decode never started");
            std::thread::sleep(Duration::from_millis(2));
        }

        service.prioritize(&[suspect_id], Priority::Background);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !service.suspects.lock.is_locked_exclusive() {
            assert!(
                Instant::now() < deadline,
                "the suspect never registered as a waiting writer"
            );
            std::thread::sleep(Duration::from_millis(2));
        }

        service.prioritize(&[ordinary_id], Priority::Visible);
        service.wait_idle();

        let started_after = ORDINARY_STARTED_AFTER
            .get()
            .expect("the ordinary photo never started decoding");
        assert!(
            *started_after < Duration::from_secs(2),
            "the ordinary decode should start well before the 3s stuck one finishes, not \
             queue behind the suspect's own wait for it; started after {started_after:?}"
        );
    }

    /// With the queue otherwise idle, the suspect from the test above must not hammer
    /// `decode_lock` once a second for as long as the stuck ordinary decode lasts: that was
    /// the bug this backoff exists to fix (two warnings a second, forever, for a merely slow
    /// decode). Same construction as above - queued one step at a time, each only once the
    /// previous one actually happened - but the hang decode here runs long enough (6s) that
    /// the suspect can never succeed inside the measurement window, so every attempt in that
    /// window is a timeout attributable purely to the backoff schedule: one immediately (at
    /// the suspect's own first pop) and, after `SUSPECT_WAIT` fails, one no earlier than
    /// `SUSPECT_BACKOFF_START` later - two attempts in a 4.5s window, never the four or five
    /// the pre-fix code (retrying every ~`SUSPECT_WAIT`, back to back) would have produced.
    #[test]
    fn a_backed_off_suspect_does_not_hammer_the_lock() {
        static HANG_STARTED: OnceLock<()> = OnceLock::new();

        fn probe(
            cache: &ThumbCache,
            source: &Path,
            orientation: u8,
            edit: Edit,
        ) -> Result<(DynamicImage, DynamicImage)> {
            if source.to_string_lossy().contains("hang") {
                let _ = HANG_STARTED.set(());
                std::thread::sleep(Duration::from_secs(6));
            }
            default_render(cache, source, orientation, edit)
        }

        let (_dir, lib, cache, ids) = setup(&[
            ("a_hang.jpg", jpeg_bytes(40, 20)),
            ("b_suspect.jpg", jpeg_bytes(40, 20)),
        ]);
        let (hang_id, suspect_id) = (ids[0], ids[1]);
        let suspect_key = item_key(&lib, suspect_id);
        let inflight = InFlight::new(cache.root());
        std::mem::forget(inflight.begin(suspect_id, suspect_key));
        inflight.recover();
        // See `one_death_is_forgiven`'s comment: only one `InFlight` at a time can hold the
        // cache root's lock file.
        drop(inflight);

        let service = ThumbService::start_with(lib.clone(), cache, 2, probe);

        service.prioritize(&[hang_id], Priority::Background);
        let deadline = Instant::now() + Duration::from_secs(5);
        while HANG_STARTED.get().is_none() {
            assert!(Instant::now() < deadline, "the hang decode never started");
            std::thread::sleep(Duration::from_millis(2));
        }

        service.prioritize(&[suspect_id], Priority::Background);
        std::thread::sleep(Duration::from_millis(4_500));

        let attempts = service
            .suspects
            .backoff
            .attempts
            .load(std::sync::atomic::Ordering::SeqCst);
        assert!(
            attempts <= 2,
            "the suspect attempted the write lock {attempts} times in 4.5s; the pre-backoff \
             code attempted roughly once a second, so this would have been 4 or 5"
        );

        // Let the hang decode finish so the service can be dropped cleanly.
        service.wait_idle();
    }

    /// The reviewer's exact defeat of the backoff: `request`'s own retries readmitting a
    /// delayed suspect early. Same construction as the hammer test above - the hang decode's
    /// render has actually started before the suspect is even queued - but here the suspect
    /// is left to time out exactly once on its own (matching the "a suspect that has timed
    /// out once" setup), and then `request` is called for it twice, the way `protocol.rs`'s
    /// `thumb` handler would on two separate 503-triggered UI retries.
    ///
    /// Before the fix, each `request` call pushed the suspect back into `order` immediately
    /// (`State::push` had no guard against readmitting a delayed id), and a free worker would
    /// pop it and burn another `SUSPECT_WAIT` finding the write lock still held by the hang
    /// decode - about 1s and one fresh `decode_lock` attempt per call, on top of the one from
    /// setup. With the fix, that same `push` leaves the suspect exactly where it was, so
    /// `wait_for` finds it neither queued nor in flight and returns at once - no worker is
    /// ever woken for it, so `request` runs both of its rounds without ever waiting, and
    /// returns `ThumbUnavailable`.
    ///
    /// Probe: revert `State::push`'s delayed check (as in the queue-level test) and this goes
    /// RED - not on the loop's own elapsed-time assertions (`wait_for` still returns quickly
    /// either way, since it only reads `entries`/`in_flight`, not `delayed`, so promptness by
    /// itself isn't what distinguishes the bug), but on the final attempts-count assertion
    /// below: a free worker picks up the wrongly-readmitted job and burns another
    /// `SUSPECT_WAIT` (up to 1s) attempting the lock, so the fixed-length wait after the loop
    /// needs to be longer than that to reliably observe it - shorter windows (tried down to
    /// 300ms) let the assertion complete before the attempt lands and pass by accident.
    #[test]
    fn a_request_for_an_already_delayed_suspect_returns_promptly_without_a_new_attempt() {
        static HANG_STARTED: OnceLock<()> = OnceLock::new();

        fn probe(
            cache: &ThumbCache,
            source: &Path,
            orientation: u8,
            edit: Edit,
        ) -> Result<(DynamicImage, DynamicImage)> {
            if source.to_string_lossy().contains("hang") {
                let _ = HANG_STARTED.set(());
                std::thread::sleep(Duration::from_secs(6));
            }
            default_render(cache, source, orientation, edit)
        }

        let (_dir, lib, cache, ids) = setup(&[
            ("a_hang.jpg", jpeg_bytes(40, 20)),
            ("b_suspect.jpg", jpeg_bytes(40, 20)),
        ]);
        let (hang_id, suspect_id) = (ids[0], ids[1]);
        let suspect_key = item_key(&lib, suspect_id);
        let inflight = InFlight::new(cache.root());
        std::mem::forget(inflight.begin(suspect_id, suspect_key));
        inflight.recover();
        // See `one_death_is_forgiven`'s comment: only one `InFlight` at a time can hold the
        // cache root's lock file.
        drop(inflight);

        let service = ThumbService::start_with(lib.clone(), cache, 2, probe);

        service.prioritize(&[hang_id], Priority::Background);
        let deadline = Instant::now() + Duration::from_secs(5);
        while HANG_STARTED.get().is_none() {
            assert!(Instant::now() < deadline, "the hang decode never started");
            std::thread::sleep(Duration::from_millis(2));
        }

        // Let the suspect make exactly its one setup timeout, the way the hammer test's own
        // sleep does, but stop as soon as it has happened rather than waiting out a fixed
        // duration - this is standing in for "a suspect that has timed out once".
        service.prioritize(&[suspect_id], Priority::Background);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if service
                .suspects
                .backoff
                .attempts
                .load(std::sync::atomic::Ordering::SeqCst)
                >= 1
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the suspect never made its first attempt"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let attempts_after_setup = service
            .suspects
            .backoff
            .attempts
            .load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            attempts_after_setup, 1,
            "exactly the one attempt from the setup timeout"
        );

        for round in 0..2 {
            let start = Instant::now();
            assert!(
                matches!(
                    service.request(suspect_id, ThumbSize::Grid, Duration::from_secs(10)),
                    Err(Error::ThumbUnavailable(_))
                ),
                "round {round}"
            );
            assert!(
                start.elapsed() < Duration::from_millis(500),
                "round {round} took {:?} - should return promptly while backing off, not wait \
                 out another SUSPECT_WAIT",
                start.elapsed()
            );
        }

        // Longer than one `SUSPECT_WAIT`: on reverted code a free worker would pick up the
        // wrongly-readmitted job and take up to a second finding the write lock still held
        // by the hang decode before `bump` runs - too short a wait here would let this
        // assertion pass by accident, exactly the failure mode this comment on the test
        // itself exists to head off. On the fixed code nothing is running for this id at
        // all, so the wait only ever costs time, never correctness in either direction.
        std::thread::sleep(Duration::from_millis(1200));
        assert_eq!(
            service
                .suspects
                .backoff
                .attempts
                .load(std::sync::atomic::Ordering::SeqCst),
            attempts_after_setup,
            "neither request call made a fresh decode_lock attempt"
        );

        // Let the hang decode (and the suspect's eventual retry behind it) finish so the
        // service can be dropped cleanly.
        service.wait_idle();
    }

    /// `SuspectBackoff::reset` (via `Suspects::resolved`) is a named requirement on its own:
    /// winning `decode_lock` must clear the step count, not just leave the id delayed until
    /// its next timeout. A suspect times out once against a blocking ordinary decode (steps
    /// == 1), the blocking decode then finishes, and the suspect's own retry wins the lock -
    /// asserted directly on `SuspectBackoff.steps` rather than by inference from timing, so
    /// this is pinned regardless of how `resolved`'s other effects are implemented.
    ///
    /// Probe: replace `Suspects::resolved`'s body with a no-op (or drop the `Some(guard)`
    /// branch's call to it) and this goes RED - `steps` still holds an entry for
    /// `suspect_id` after it reached `Ready`.
    #[test]
    fn winning_the_lock_resets_the_suspects_backoff() {
        static HANG_STARTED: OnceLock<()> = OnceLock::new();

        fn probe(
            cache: &ThumbCache,
            source: &Path,
            orientation: u8,
            edit: Edit,
        ) -> Result<(DynamicImage, DynamicImage)> {
            if source.to_string_lossy().contains("hang") {
                let _ = HANG_STARTED.set(());
                // Long enough that the suspect's first `try_write_for(SUSPECT_WAIT)` (1s)
                // times out while this is still running, short enough that it has finished
                // well before the suspect's backed-off retry (>= SUSPECT_BACKOFF_START = 2s
                // after the timeout) comes around, so that retry finds the lock free.
                std::thread::sleep(Duration::from_millis(1_500));
            }
            default_render(cache, source, orientation, edit)
        }

        let (_dir, lib, cache, ids) = setup(&[
            ("a_hang.jpg", jpeg_bytes(40, 20)),
            ("b_suspect.jpg", jpeg_bytes(40, 20)),
        ]);
        let (hang_id, suspect_id) = (ids[0], ids[1]);
        let suspect_key = item_key(&lib, suspect_id);
        let inflight = InFlight::new(cache.root());
        std::mem::forget(inflight.begin(suspect_id, suspect_key));
        inflight.recover();
        // See `one_death_is_forgiven`'s comment: only one `InFlight` at a time can hold the
        // cache root's lock file.
        drop(inflight);

        let service = ThumbService::start_with(lib.clone(), cache, 2, probe);

        service.prioritize(&[hang_id], Priority::Background);
        let deadline = Instant::now() + Duration::from_secs(5);
        while HANG_STARTED.get().is_none() {
            assert!(Instant::now() < deadline, "the hang decode never started");
            std::thread::sleep(Duration::from_millis(2));
        }

        service.prioritize(&[suspect_id], Priority::Background);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if service
                .suspects
                .backoff
                .attempts
                .load(std::sync::atomic::Ordering::SeqCst)
                >= 1
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the suspect never made its first attempt"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            service.suspects.backoff.steps.lock().get(&suspect_id),
            Some(&1),
            "one recorded timeout before the blocking decode has even finished"
        );

        // Let the hang decode finish and the suspect's backed-off retry win the lock.
        service.wait_idle();

        assert_eq!(state(&lib, suspect_id), ThumbState::Ready);
        assert!(
            service
                .suspects
                .backoff
                .steps
                .lock()
                .get(&suspect_id)
                .is_none(),
            "the step count must not outlive winning the lock"
        );
        assert_eq!(
            service.suspects.backoff.bump(suspect_id),
            SUSPECT_BACKOFF_START,
            "confirms the reset from a second angle: a fresh timeout starts at the short wait"
        );
    }

    /// `deaths` (and so which decode path an id takes) is keyed by the item's *current*
    /// `thumb_key()`, but `SuspectBackoff`'s step count is keyed by id alone - so an edit
    /// that changes the key can leave a stale step count (and, until its deadline, a stale
    /// `ThumbQueue::delayed` entry) attached to an id that is not a suspect for its new key
    /// at all. A suspect times out once (steps == 1) against a blocking decode; before its
    /// backed-off retry comes due, the photo is edited - a new key with no death record, so
    /// its eventual redecode takes the *ordinary* path, never touching `decode_lock`. That
    /// ordinary path must still notice and clear the leftover step count.
    ///
    /// Probe: remove the `if suspects.backoff.has_steps(id) { suspects.resolved(id, queue); }`
    /// guard from `process`'s ordinary branch and this goes RED - `steps` still holds an
    /// entry for `suspect_id` after the edited photo reaches `Ready`.
    #[test]
    fn an_edit_clears_a_stale_backoff_left_by_the_photos_old_key() {
        static HANG_STARTED: OnceLock<()> = OnceLock::new();

        fn probe(
            cache: &ThumbCache,
            source: &Path,
            orientation: u8,
            edit: Edit,
        ) -> Result<(DynamicImage, DynamicImage)> {
            if source.to_string_lossy().contains("hang") {
                let _ = HANG_STARTED.set(());
                // Long enough for the suspect's one `try_write_for` to time out, short
                // enough to be done well before the edit is applied and the suspect's
                // (now ordinary) redecode is due - see `winning_the_lock_resets...` above.
                std::thread::sleep(Duration::from_millis(1_500));
            }
            default_render(cache, source, orientation, edit)
        }

        let (_dir, lib, cache, ids) = setup(&[
            ("a_hang.jpg", jpeg_bytes(40, 20)),
            ("b_suspect.jpg", jpeg_bytes(40, 20)),
        ]);
        let (hang_id, suspect_id) = (ids[0], ids[1]);
        let suspect_key = item_key(&lib, suspect_id);
        let inflight = InFlight::new(cache.root());
        std::mem::forget(inflight.begin(suspect_id, suspect_key));
        inflight.recover();
        // See `one_death_is_forgiven`'s comment: only one `InFlight` at a time can hold the
        // cache root's lock file.
        drop(inflight);

        let service = ThumbService::start_with(lib.clone(), cache, 2, probe);

        service.prioritize(&[hang_id], Priority::Background);
        let deadline = Instant::now() + Duration::from_secs(5);
        while HANG_STARTED.get().is_none() {
            assert!(Instant::now() < deadline, "the hang decode never started");
            std::thread::sleep(Duration::from_millis(2));
        }

        service.prioritize(&[suspect_id], Priority::Background);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if service
                .suspects
                .backoff
                .attempts
                .load(std::sync::atomic::Ordering::SeqCst)
                >= 1
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the suspect never made its first attempt"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            service.suspects.backoff.steps.lock().get(&suspect_id),
            Some(&1),
            "one recorded timeout, well before the edit below"
        );

        // A turn changes `edit_turns`, part of `thumb_key()`: the redecode this schedules
        // has a brand new key with no death record at all, so it is not a suspect.
        lib.set_item_edit(suspect_id, Edit::new(1, None).unwrap())
            .unwrap();

        // Let the hang decode finish, the deferred (stale) entry become due, and the now-
        // ordinary redecode run.
        service.wait_idle();

        assert_eq!(state(&lib, suspect_id), ThumbState::Ready);
        assert!(
            service
                .suspects
                .backoff
                .steps
                .lock()
                .get(&suspect_id)
                .is_none(),
            "the old key's step count must not survive an edit that gave it a new one"
        );
    }

    /// A photo whose thumbnail is already cached - a rescan of an unchanged file, say - has
    /// no decode to run, so it must never touch `decode_lock` or write a marker for one,
    /// let alone reach `render`. Queued alongside an ordinary decode that holds `decode_lock`
    /// for well past the point this one should have finished, a version that still took the
    /// lock (a suspect does, exclusively, since one death is recorded against it below) would
    /// queue behind that decode; skipping the lock entirely finishes near-instantly instead.
    /// `must_not_render` on top proves `render` itself is never reached, which was already
    /// true before this fix (`process_item`'s own `is_complete` check saw to that) - the
    /// timing bound below is what actually distinguishes "skipped" from "took the lock, found
    /// nothing to render, gave it back".
    ///
    /// Made deterministic the same way as the stall test above, and for the same reason: with
    /// both jobs pushed together, `is_complete`'s early return makes the exact pop order
    /// unobservable on the fixed code either way, so on the *old* code (no early return) this
    /// would pass whenever the cached photo happened to be popped, and its `try_write_for`
    /// happened to run, before the hang decode was actually holding `decode_lock` - nothing
    /// would be blocking it yet. Queuing the hang photo alone, waiting for its render to
    /// actually start (not merely to be popped), and only then queuing the cached one forces
    /// the ordering that makes the old code's bug observable every run: `decode_lock` is
    /// genuinely held as a reader by the time the cached photo is ever considered.
    #[test]
    fn an_already_cached_photo_skips_the_marker_and_the_lock() {
        static HANG_STARTED: OnceLock<()> = OnceLock::new();

        fn probe(
            cache: &ThumbCache,
            source: &Path,
            orientation: u8,
            edit: Edit,
        ) -> Result<(DynamicImage, DynamicImage)> {
            if source.to_string_lossy().contains("hang") {
                let _ = HANG_STARTED.set(());
                std::thread::sleep(Duration::from_millis(300));
                default_render(cache, source, orientation, edit)
            } else {
                panic!("the already-cached photo's decoder must never run");
            }
        }

        let (dir, lib, cache, ids) = setup(&[
            ("a_hang.jpg", jpeg_bytes(40, 20)),
            ("b_cached.jpg", jpeg_bytes(40, 20)),
        ]);
        let (hang_id, cached_id) = (ids[0], ids[1]);
        let item = lib.item(cached_id).unwrap().unwrap();
        let key = item.thumb_key();
        cache
            .generate(Path::new(&item.path), item.orientation, key)
            .unwrap();
        assert!(cache.is_complete(key));

        let inflight = InFlight::new(cache.root());
        std::mem::forget(inflight.begin(cached_id, key));
        inflight.recover();
        assert_eq!(inflight.deaths(cached_id, key), 1);
        // See `one_death_is_forgiven`'s comment: only one `InFlight` at a time can hold the
        // cache root's lock file.
        drop(inflight);

        let service = ThumbService::start_with(lib.clone(), cache, 2, probe);

        service.prioritize(&[hang_id], Priority::Background);
        let deadline = Instant::now() + Duration::from_secs(5);
        while HANG_STARTED.get().is_none() {
            assert!(Instant::now() < deadline, "the hang decode never started");
            std::thread::sleep(Duration::from_millis(2));
        }

        let start = Instant::now();
        service.prioritize(&[cached_id], Priority::Visible);

        let mut became_ready_after = None;
        while start.elapsed() < Duration::from_secs(2) {
            if state(&lib, cached_id) == ThumbState::Ready {
                became_ready_after = Some(start.elapsed());
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        service.wait_idle();

        let became_ready_after = became_ready_after.expect("the cached photo never reached Ready");
        assert!(
            became_ready_after < Duration::from_millis(200),
            "took {became_ready_after:?} - should not have waited on decode_lock behind the \
             hung ordinary decode at all"
        );
        assert!(
            !marker(&dir, cached_id).exists(),
            "no decode ran, so no marker"
        );
        assert!(
            !death_record(&dir, cached_id).exists(),
            "the record is resolved - Ready - not left standing"
        );
    }
}
