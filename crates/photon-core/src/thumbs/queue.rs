use parking_lot::{Condvar, Mutex};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::{Duration, Instant};

/// Lower sorts first: visible grid cells beat viewer neighbours beat background fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    Visible,
    Neighbour,
    Background,
}

#[derive(Default)]
struct State {
    /// (priority, insertion sequence, item id) — pop_first yields the next job.
    order: BTreeSet<(Priority, u64, i64)>,
    entries: HashMap<i64, (Priority, u64)>,
    visible: Vec<i64>,
    next_seq: u64,
    in_flight: HashSet<i64>,
    /// Pushes that arrived while their id was in flight, applied when the job finishes.
    ///
    /// Queueing one alongside the running job would have a second worker decode the same
    /// photo, but dropping it loses a retry nobody makes again: `enqueue_pending` pushes
    /// each item left `Pending` by a transient I/O error exactly once, with none of
    /// `request`'s rounds behind it. Re-running a job that did succeed costs a stat of the
    /// cache file, since `process_item` short-circuits on a complete fingerprint.
    deferred: HashMap<i64, Priority>,
    /// Number of live `wait` futures per id. An id someone is waiting on is demoted no
    /// further than `Neighbour`: it may have scrolled out of the strictly-visible span while
    /// its request was still in the queue, and dropping it to `Background` would park it
    /// behind the whole backlog until the waiter times out.
    ///
    /// A floor rather than an exemption, because the set is unbounded in both count and
    /// time. A fast scroll abandons one request per tile it passes and the webview gives us
    /// no cancellation signal, so hundreds of ids can sit here at `Visible` with low seqs for
    /// the full request timeout. Exempting them outranks the tiles that are actually on
    /// screen, and the workers decode invisible photos while the grid stays empty.
    waiters: HashMap<i64, usize>,
    /// The wakers of the `WaitFor`s parked on each id, by their token, woken only by that
    /// id's `done` (or `close`). Per id rather than one broadcast: a waiter parks for up to
    /// the whole request timeout, hundreds can be parked at once during a fast scroll, and
    /// waking every one of them on every finished job only for all but one to re-check and
    /// park again is work that grows with the backlog it is meant to serve.
    wakers: HashMap<i64, Vec<(u64, Waker)>>,
    /// The next `WaitFor`'s token, which is how a re-polled waiter replaces its own waker
    /// rather than adding another.
    next_waiter: u64,
    closed: bool,
    /// Jobs backed off by `defer`, not eligible to be popped until their `Instant` passes -
    /// a suspect that lost the race for `decode_lock` and is waiting out its back-off rather
    /// than an id that is merely low priority. Not `order`: something in `order` is ready to
    /// run the moment a worker is free, where a delayed job must not be handed to a worker at
    /// all until its time comes, or a worker would just spin popping it and re-deferring it
    /// with nothing else to do in between - see `pop_blocking`, which waits on the earliest
    /// one rather than polling.
    delayed: Vec<(Instant, i64, Priority)>,
    /// Set by `note_ready` - a job made a photo's thumbnail ready - and cleared by the
    /// `wait_drained` that reports it. See `wait_drained`.
    readied: bool,
    /// When the last job finished, which is when the queue last stopped being busy: what
    /// `wait_drained` measures its quiet from.
    last_done: Option<Instant>,
}

impl State {
    /// Nothing queued, in flight, or backed off waiting for its delay - see `wait_idle`.
    fn idle(&self) -> bool {
        self.order.is_empty() && self.in_flight.is_empty() && self.delayed.is_empty()
    }

    fn push(&mut self, id: i64, priority: Priority) {
        if self.in_flight.contains(&id) {
            let held = self.deferred.entry(id).or_insert(priority);
            *held = (*held).min(priority);
            return;
        }
        if let Some(entry) = self
            .delayed
            .iter_mut()
            .find(|&&mut (_, existing, _)| existing == id)
        {
            // Already backing off: a push while it's delayed updates the priority it will
            // run at once eligible (never lowering it - a Visible request must still beat
            // a Background one when the deadline arrives), but never readmits it early.
            // Ignoring `delayed` here is exactly what let `request`'s own retries, and
            // `set_visible`/`prioritize`/`enqueue_pending`, defeat the backoff by popping
            // the id again the moment they ran - see `SUSPECT_BACKOFF_START`.
            entry.2 = entry.2.min(priority);
            return;
        }
        if let Some(&(current, seq)) = self.entries.get(&id) {
            if current <= priority {
                return;
            }
            self.order.remove(&(current, seq, id));
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.order.insert((priority, seq, id));
        self.entries.insert(id, (priority, seq));
    }

    fn demote(&mut self, id: i64, priority: Priority) {
        let priority = if self.waiters.contains_key(&id) {
            priority.min(Priority::Neighbour)
        } else {
            priority
        };
        if let Some(&(current, seq)) = self.entries.get(&id)
            && current < priority
        {
            self.order.remove(&(current, seq, id));
            self.order.insert((priority, seq, id));
            self.entries.insert(id, (priority, seq));
        }
        // A backing-off suspect is held here rather than in `order`, and `admit_due` pushes
        // it back at whatever this says - left alone, a tile scrolled away returns from its
        // back-off still `Visible`, ahead of the tiles actually on screen.
        if let Some(entry) = self
            .delayed
            .iter_mut()
            .find(|&&mut (_, existing, _)| existing == id)
        {
            entry.2 = entry.2.max(priority);
        }
        // Likewise a push that arrived while the job was running, which `done` queues at
        // whatever this holds.
        if let Some(held) = self.deferred.get_mut(&id) {
            *held = (*held).max(priority);
        }
    }

    fn done(&mut self, id: i64) {
        self.last_done = Some(Instant::now());
        self.in_flight.remove(&id);
        if let Some(priority) = self.deferred.remove(&id) {
            self.push(id, priority);
        }
    }

    /// Queued or being processed: what a `WaitFor` waits out.
    fn busy(&self, id: i64) -> bool {
        self.entries.contains_key(&id) || self.in_flight.contains(&id)
    }

    /// The wakers parked on `id`, to be woken once the lock is released. Taken whether or
    /// not `id` is still busy - a push held while its job ran is queued again by `done` -
    /// since each waiter re-checks and re-registers on its own poll.
    fn take_wakers(&mut self, id: i64) -> Vec<Waker> {
        self.wakers
            .remove(&id)
            .map(|parked| parked.into_iter().map(|(_, waker)| waker).collect())
            .unwrap_or_default()
    }

    fn pop(&mut self) -> Option<i64> {
        let (_, _, id) = self.order.pop_first()?;
        self.entries.remove(&id);
        Some(id)
    }

    /// Backs `id` off until `not_before`. `defer` is only called from inside a job already
    /// popped and in flight, so `id` is already absent from `order` and `entries` by the
    /// time this runs - there is nothing here to remove from either. Any existing delay for
    /// the same id is replaced rather than duplicated, so a job that keeps timing out doesn't
    /// accumulate one entry per attempt.
    fn delay(&mut self, id: i64, priority: Priority, not_before: Instant) {
        self.delayed.retain(|&(_, existing, _)| existing != id);
        self.delayed.push((not_before, id, priority));
    }

    /// Drops any pending backoff for `id` with no replacement - see `ThumbQueue::forget`.
    /// Reports whether anything was actually removed, so a caller doesn't have to wake
    /// every waiter over a call that changed nothing.
    fn forget(&mut self, id: i64) -> bool {
        let before = self.delayed.len();
        self.delayed.retain(|&(_, existing, _)| existing != id);
        self.delayed.len() != before
    }

    /// Moves every delayed job whose time has come into `order`, where `pop` can find it.
    fn admit_due(&mut self, now: Instant) {
        let (due, still_delayed): (Vec<_>, Vec<_>) = self
            .delayed
            .drain(..)
            .partition(|&(when, _, _)| when <= now);
        self.delayed = still_delayed;
        for (_, id, priority) in due {
            self.push(id, priority);
        }
    }

    /// The earliest time any delayed job becomes eligible, or `None` if there are none -
    /// what `pop_blocking` and `wait_idle` wake up for instead of polling.
    fn next_delayed(&self) -> Option<Instant> {
        self.delayed.iter().map(|&(when, _, _)| when).min()
    }
}

/// Thumbnail job queue shared by the worker pool. Tracks in-flight jobs so callers can wait for idle.
#[derive(Default)]
pub struct ThumbQueue {
    state: Mutex<State>,
    // One condvar serves both workers and idle-waiters, so every change uses notify_all.
    changed: Condvar,
}

impl ThumbQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, id: i64, priority: Priority) {
        self.state.lock().push(id, priority);
        self.changed.notify_all();
    }

    pub fn push_many(&self, ids: &[i64], priority: Priority) {
        let mut state = self.state.lock();
        for &id in ids {
            state.push(id, priority);
        }
        drop(state);
        self.changed.notify_all();
    }

    /// Replaces the set of items currently on screen.
    pub fn set_visible(&self, ids: &[i64]) {
        let mut state = self.state.lock();
        for id in std::mem::take(&mut state.visible) {
            state.demote(id, Priority::Background);
        }
        for &id in ids {
            state.push(id, Priority::Visible);
        }
        state.visible = ids.to_vec();
        drop(state);
        self.changed.notify_all();
    }

    /// Blocks until a job is available. Every `Some(id)` must be followed by `done(id)`.
    ///
    /// A delayed job (see `defer`) is never handed out before its time: rather than polling,
    /// a worker with nothing else to do waits on the condvar with the earliest delayed job's
    /// deadline, waking (via `wait_until`'s timeout, or a `notify_all` from a `push`/`defer`
    /// elsewhere) to recheck rather than sleeping past it or spinning before it.
    pub fn pop_blocking(&self) -> Option<i64> {
        let mut state = self.state.lock();
        loop {
            if state.closed {
                return None;
            }
            state.admit_due(Instant::now());
            if let Some(id) = state.pop() {
                state.in_flight.insert(id);
                return Some(id);
            }
            match state.next_delayed() {
                Some(deadline) => {
                    self.changed.wait_until(&mut state, deadline);
                }
                None => self.changed.wait(&mut state),
            }
        }
    }

    /// `pop_blocking`, but giving up at `deadline`: for a consumer that is a person's
    /// webview asking over IPC, not a worker thread that lives as long as the queue. Every
    /// `Some(id)` must be followed by `done(id)`, as for `pop_blocking`.
    pub fn pop_until(&self, deadline: Instant) -> Option<i64> {
        let mut state = self.state.lock();
        loop {
            if state.closed {
                return None;
            }
            state.admit_due(Instant::now());
            if let Some(id) = state.pop() {
                state.in_flight.insert(id);
                return Some(id);
            }
            if Instant::now() >= deadline {
                return None;
            }
            let wake = state.next_delayed().map_or(deadline, |d| d.min(deadline));
            self.changed.wait_until(&mut state, wake);
        }
    }

    /// Backs `id` off until `not_before`: taken out of contention for a worker until then, but
    /// not dropped - see `State::delay`. Used for a suspect that lost the race for
    /// `decode_lock`, so it doesn't retry immediately and doesn't need a worker sleeping to
    /// wait it out.
    pub fn defer(&self, id: i64, priority: Priority, not_before: Instant) {
        self.state.lock().delay(id, priority, not_before);
        self.changed.notify_all();
    }

    /// Drops `id`'s pending backoff, if it has one - see `Suspects::resolved`. A no-op for
    /// a job reached through the queue itself: `admit_due` already moved `id` out of
    /// `delayed` before `pop` ever handed it to a worker, so there is nothing here to
    /// remove. The one caller that can actually find something to remove is
    /// `ThumbService::get_or_generate`, which resolves an id by calling `process` directly,
    /// bypassing the queue and its bookkeeping entirely - test-only today (see its own
    /// doc), but cheap enough to guard against regardless.
    pub fn forget(&self, id: i64) {
        if self.state.lock().forget(id) {
            self.changed.notify_all();
        }
    }

    /// Marks the popped job `id` finished and wakes idle-waiters, and the waiters on `id`.
    pub fn done(&self, id: i64) {
        let mut state = self.state.lock();
        state.done(id);
        let wakers = state.take_wakers(id);
        drop(state);
        self.changed.notify_all();
        wakers.into_iter().for_each(Waker::wake);
    }

    /// `done`, for a job that settled `id`'s fate, dropping any push held while it ran
    /// rather than queueing it. A worker's re-run of a settled job costs a stat and ends at
    /// once; a video's waits for the webview to poll again, and a `wait` whose
    /// own push was held - a thumbnail requested while the frame was being drawn - would
    /// wait with it, for a frame that is already in the cache.
    pub fn done_settled(&self, id: i64) {
        let mut state = self.state.lock();
        state.deferred.remove(&id);
        state.done(id);
        let wakers = state.take_wakers(id);
        drop(state);
        self.changed.notify_all();
        wakers.into_iter().for_each(Waker::wake);
    }

    /// Waits until nothing is queued, in flight, or backed off waiting for its delay to pass -
    /// a deferred suspect is still work outstanding, even though no worker is holding it right
    /// now, so this has to wait it out the same way `pop_blocking` does rather than declaring
    /// idle as soon as `order` and `in_flight` are both empty.
    pub fn wait_idle(&self) {
        let mut state = self.state.lock();
        loop {
            state.admit_due(Instant::now());
            if state.closed || state.idle() {
                return;
            }
            match state.next_delayed() {
                Some(deadline) => {
                    self.changed.wait_until(&mut state, deadline);
                }
                None => self.changed.wait(&mut state),
            }
        }
    }

    /// Records that the running job made a photo's thumbnail ready: a new picture in the
    /// cache for the look-alike pass to hash. Called by the worker before its `done`, which
    /// is what wakes `wait_drained`.
    pub fn note_ready(&self) {
        self.state.lock().readied = true;
        self.changed.notify_all();
    }

    /// Blocks until some job since the last call has made a thumbnail ready (`note_ready`)
    /// and the queue has then stayed idle for `settle` since its last job finished, and
    /// returns true; or returns false once the queue closes.
    ///
    /// A debounce, not a per-thumbnail signal: what it paces is a whole-library look-alike
    /// pass. A scroll renders the visible tiles in bursts and an import renders for as long
    /// as it lasts, and either answers once, after its work has stopped.
    pub fn wait_drained(&self, settle: Duration) -> bool {
        let mut state = self.state.lock();
        loop {
            if state.closed {
                return false;
            }
            let now = Instant::now();
            state.admit_due(now);
            if state.readied && state.idle() {
                let quiet_until = state.last_done.unwrap_or(now) + settle;
                if now >= quiet_until {
                    state.readied = false;
                    return true;
                }
                self.changed.wait_until(&mut state, quiet_until);
                continue;
            }
            match state.next_delayed() {
                Some(deadline) => {
                    self.changed.wait_until(&mut state, deadline);
                }
                None => self.changed.wait(&mut state),
            }
        }
    }

    /// A future that is ready once `id` is neither queued nor being processed, or the queue
    /// closes. It has no deadline of its own: the caller bounds it, and dropping it is how a
    /// wait gives up.
    ///
    /// For as long as it exists, `id` cannot be demoted past `Neighbour` (see
    /// `State::waiters`); the protection goes with the future, however it ends - ready,
    /// timed out, or its request abandoned - so a later `set_visible` demotes the id
    /// normally once nobody is waiting for it.
    ///
    /// Awaiting it holds no thread. The `photon://` handler awaits one per thumbnail not
    /// built yet, for up to `THUMB_TIMEOUT`; parked on a blocking thread instead, a fast
    /// scroll's worth of them filled Tokio's blocking pool and queued every cached thumbnail,
    /// full-size image and blocking IPC command behind waits that were only going to time out.
    pub fn wait(&self, id: i64) -> WaitFor<'_> {
        let mut state = self.state.lock();
        *state.waiters.entry(id).or_insert(0) += 1;
        let token = state.next_waiter;
        state.next_waiter += 1;
        WaitFor {
            queue: self,
            id,
            token,
        }
    }

    /// `wait`, blocking this thread until `id` is done, the queue closes, or `deadline`
    /// passes. Returns false only on timeout.
    pub fn wait_for(&self, id: i64, deadline: Instant) -> bool {
        block_on_until(self.wait(id), deadline).is_some() || !self.state.lock().busy(id)
    }

    pub fn len(&self) -> usize {
        self.state.lock().order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn close(&self) {
        let mut state = self.state.lock();
        state.closed = true;
        let wakers: Vec<Waker> = state
            .wakers
            .drain()
            .flat_map(|(_, parked)| parked.into_iter().map(|(_, waker)| waker))
            .collect();
        drop(state);
        self.changed.notify_all();
        wakers.into_iter().for_each(Waker::wake);
    }
}

/// See `ThumbQueue::wait`.
#[must_use = "a wait does nothing unless awaited"]
pub struct WaitFor<'a> {
    queue: &'a ThumbQueue,
    id: i64,
    token: u64,
}

impl Future for WaitFor<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut state = self.queue.state.lock();
        if state.closed || !state.busy(self.id) {
            return Poll::Ready(());
        }
        // Registered under the lock that `done` takes its wakers under, so a job finishing
        // between the check above and this line cannot be missed.
        let parked = state.wakers.entry(self.id).or_default();
        match parked.iter_mut().find(|(token, _)| *token == self.token) {
            Some((_, waker)) => waker.clone_from(cx.waker()),
            None => parked.push((self.token, cx.waker().clone())),
        }
        Poll::Pending
    }
}

impl Drop for WaitFor<'_> {
    fn drop(&mut self) {
        let mut state = self.queue.state.lock();
        if let std::collections::hash_map::Entry::Occupied(mut parked) = state.wakers.entry(self.id)
        {
            parked.get_mut().retain(|(token, _)| *token != self.token);
            if parked.get().is_empty() {
                parked.remove();
            }
        }
        if let std::collections::hash_map::Entry::Occupied(mut waiters) =
            state.waiters.entry(self.id)
        {
            *waiters.get_mut() -= 1;
            if *waiters.get() == 0 {
                waiters.remove();
            }
        }
    }
}

/// Runs `future` on this thread until it is ready, or gives up at `deadline` and drops it.
/// What a caller on a plain thread - a test, or anything outside an async runtime - uses to
/// await the queue's futures.
pub(crate) fn block_on_until<F: Future>(future: F, deadline: Instant) -> Option<F::Output> {
    struct Unpark(std::thread::Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(out) = future.as_mut().poll(&mut cx) {
            return Some(out);
        }
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        // A wake between the poll and here leaves the thread's token set, so this returns
        // at once rather than sleeping through it.
        std::thread::park_timeout(deadline - now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    impl ThumbQueue {
        fn has_waiter(&self, id: i64) -> bool {
            self.state.lock().waiters.contains_key(&id)
        }

        fn parked_wakers(&self) -> usize {
            self.state.lock().wakers.values().map(Vec::len).sum()
        }

        fn delayed_priority(&self, id: i64) -> Option<Priority> {
            self.state
                .lock()
                .delayed
                .iter()
                .find(|&&(_, existing, _)| existing == id)
                .map(|&(_, _, priority)| priority)
        }
    }

    fn drain(q: &ThumbQueue) -> Vec<i64> {
        let mut out = Vec::new();
        while !q.is_empty() {
            let id = q.pop_blocking().unwrap();
            out.push(id);
            q.done(id);
        }
        out
    }

    #[test]
    fn serves_by_priority_then_fifo() {
        let q = ThumbQueue::new();
        q.push_many(&[1, 2], Priority::Background);
        q.push(3, Priority::Neighbour);
        q.push(4, Priority::Visible);
        assert_eq!(drain(&q), [4, 3, 1, 2]);
    }

    #[test]
    fn push_only_raises_priority() {
        let q = ThumbQueue::new();
        q.push(1, Priority::Background);
        q.push(2, Priority::Visible);
        q.push(2, Priority::Background); // ignored: already higher
        q.push(1, Priority::Neighbour); // raised
        assert_eq!(q.len(), 2);
        assert_eq!(drain(&q), [2, 1]);
    }

    #[test]
    fn set_visible_demotes_previous_visible_items() {
        let q = ThumbQueue::new();
        q.push(9, Priority::Neighbour);
        q.set_visible(&[1, 2]);
        q.set_visible(&[3]);
        assert_eq!(drain(&q), [3, 9, 1, 2]);
    }

    #[test]
    fn close_releases_blocked_poppers() {
        let q = Arc::new(ThumbQueue::new());
        let worker = {
            let q = q.clone();
            std::thread::spawn(move || q.pop_blocking())
        };
        std::thread::sleep(std::time::Duration::from_millis(50));
        q.close();
        assert_eq!(worker.join().unwrap(), None);
    }

    #[test]
    fn done_settled_drops_a_push_held_while_the_job_ran() {
        let q = ThumbQueue::new();
        q.push(1, Priority::Background);
        q.push(2, Priority::Background);
        assert_eq!(q.pop_blocking(), Some(1));
        assert_eq!(q.pop_blocking(), Some(2));
        q.push(1, Priority::Visible);
        q.push(2, Priority::Visible);
        q.done_settled(1);
        q.done(2);
        assert!(
            q.wait_for(1, Instant::now()),
            "settled: neither queued nor in flight"
        );
        assert!(
            !q.wait_for(2, Instant::now()),
            "plain done queues the held push"
        );
    }

    #[test]
    fn pop_until_gives_up_at_its_deadline_and_takes_a_job_pushed_before_it() {
        use std::time::Duration;
        let q = Arc::new(ThumbQueue::new());
        let started = Instant::now();
        assert_eq!(q.pop_until(started + Duration::from_millis(50)), None);
        assert!(started.elapsed() >= Duration::from_millis(50));

        let popper = {
            let q = q.clone();
            std::thread::spawn(move || q.pop_until(Instant::now() + Duration::from_secs(5)))
        };
        std::thread::sleep(Duration::from_millis(50));
        q.push(7, Priority::Background);
        assert_eq!(popper.join().unwrap(), Some(7));
        assert!(
            !q.wait_for(7, Instant::now()),
            "popped, so in flight until done"
        );
        q.done(7);
    }

    /// The drain signal answers once per burst, after the queue has gone quiet - never for
    /// a queue that only ran jobs that readied nothing, and not while work is still going.
    #[test]
    fn wait_drained_answers_after_a_readied_job_and_a_quiet_settle() {
        let settle = std::time::Duration::from_millis(100);
        let q = Arc::new(ThumbQueue::new());
        let waiter = {
            let q = q.clone();
            std::thread::spawn(move || {
                let answered = q.wait_drained(settle);
                (answered, Instant::now())
            })
        };

        // A job that readied nothing: an already-ready photo re-queued by a sweep.
        q.push(1, Priority::Background);
        q.done(q.pop_blocking().unwrap());
        std::thread::sleep(settle * 2);
        assert!(!waiter.is_finished(), "nothing was readied");

        // One that did, followed by more work that is still running when its settle runs out.
        // The next job is queued while this one is in flight and held in flight past the
        // settle, so the queue is never idle between them however late the sleep wakes: a
        // gap left to a sleep *inside* the settle window let a loaded macOS runner oversleep
        // it, and the queue then answered - rightly - before the assertion's `last_done`
        // existed. Held past the settle, the job is also what pins "not while work is still
        // going": a queue that answered on `readied` alone answers during it.
        q.push(2, Priority::Background);
        let id = q.pop_blocking().unwrap();
        q.note_ready();
        q.push(3, Priority::Background);
        q.done(id);
        let id = q.pop_blocking().unwrap();
        std::thread::sleep(settle * 2);
        let last_done = Instant::now();
        q.done(id);

        let (answered, at) = waiter.join().unwrap();
        assert!(answered);
        assert!(
            at >= last_done + settle,
            "answered before the queue had been quiet for the settle"
        );

        // The mark was taken: the next wait needs a new one, and a close ends it.
        let waiter = {
            let q = q.clone();
            std::thread::spawn(move || q.wait_drained(settle))
        };
        std::thread::sleep(settle * 2);
        assert!(!waiter.is_finished(), "the mark was already reported");
        q.close();
        assert!(!waiter.join().unwrap(), "a closed queue answers false");
    }

    #[test]
    fn wait_idle_waits_for_in_flight_work() {
        let q = Arc::new(ThumbQueue::new());
        q.push(1, Priority::Background);
        let worker = {
            let q = q.clone();
            std::thread::spawn(move || {
                let id = q.pop_blocking().unwrap();
                std::thread::sleep(std::time::Duration::from_millis(50));
                q.done(id);
                id
            })
        };
        q.wait_idle();
        assert!(q.is_empty());
        assert_eq!(worker.join().unwrap(), 1);
    }

    use std::time::{Duration, Instant};

    #[test]
    fn push_does_not_queue_an_item_alongside_its_running_job() {
        let q = ThumbQueue::new();
        q.push(1, Priority::Background);
        let id = q.pop_blocking().unwrap();
        q.push(1, Priority::Visible);
        assert!(q.is_empty());
        q.done(id);
        q.push(1, Priority::Visible);
        assert_eq!(q.len(), 1);
    }

    /// A render that hits a transient I/O error (a network share, an external drive) leaves
    /// its item `Pending` for the next scan's `enqueue_pending` to retry. That retry is a
    /// plain `push`, with none of `request`'s rounds behind it, so dropping it because the
    /// job is still in flight loses the retry for exactly the items most likely to need one.
    #[test]
    fn a_push_arriving_while_the_id_is_in_flight_is_applied_once_it_finishes() {
        let q = ThumbQueue::new();
        q.push(1, Priority::Background);
        let id = q.pop_blocking().unwrap();

        q.push(1, Priority::Visible);
        assert!(
            q.is_empty(),
            "not queued alongside the job already running for it"
        );

        q.done(id);
        assert_eq!(drain(&q), [1], "but not dropped either");
    }

    #[test]
    fn wait_for_returns_once_the_job_finishes() {
        let q = Arc::new(ThumbQueue::new());
        q.push(1, Priority::Visible);
        let worker = {
            let q = q.clone();
            std::thread::spawn(move || {
                let id = q.pop_blocking().unwrap();
                std::thread::sleep(Duration::from_millis(50));
                q.done(id);
            })
        };
        assert!(q.wait_for(1, Instant::now() + Duration::from_secs(5)));
        worker.join().unwrap();
    }

    #[test]
    fn wait_for_times_out_while_queued() {
        let q = ThumbQueue::new();
        q.push(1, Priority::Visible);
        assert!(!q.wait_for(1, Instant::now() + Duration::from_millis(50)));
    }

    #[test]
    fn wait_for_unknown_id_returns_immediately() {
        assert!(ThumbQueue::new().wait_for(7, Instant::now()));
    }

    /// A tile that scrolls out of the visible span while its request is still queued must
    /// not fall behind the background backlog: the waiting request would time out first.
    #[test]
    fn set_visible_demotes_an_id_with_a_waiter_no_further_than_neighbour() {
        let q = Arc::new(ThumbQueue::new());
        q.push_many(&[8, 9], Priority::Background);
        q.set_visible(&[1]);
        let waiter = {
            let q = q.clone();
            std::thread::spawn(move || q.wait_for(1, Instant::now() + Duration::from_secs(5)))
        };
        // Wait until the waiter is actually registered, then scroll 1 off screen.
        while !q.has_waiter(1) {
            std::thread::sleep(Duration::from_millis(5));
        }
        q.set_visible(&[2]);

        assert_eq!(
            drain(&q),
            [2, 1, 8, 9],
            "1 yields to the tile now on screen but still beats the background backlog"
        );
        assert!(waiter.join().unwrap());
    }

    /// A fast scroll abandons one request per tile it passes, and Tauri gives the Rust side
    /// no cancellation signal, so each one sits in `waiters` at `Visible` with a low seq for
    /// the full request timeout. If that exempts them from demotion they outrank the tiles
    /// actually on screen, and the workers decode invisible photos while the grid stays
    /// empty - exactly the starvation the priority levels exist to prevent.
    #[test]
    fn ids_with_waiters_do_not_outrank_the_tiles_now_on_screen() {
        let q = Arc::new(ThumbQueue::new());
        q.set_visible(&[1, 2]);
        let abandoned: Vec<_> = [1, 2]
            .into_iter()
            .map(|id| {
                let q = q.clone();
                std::thread::spawn(move || q.wait_for(id, Instant::now() + Duration::from_secs(5)))
            })
            .collect();
        while !q.has_waiter(1) || !q.has_waiter(2) {
            std::thread::sleep(Duration::from_millis(5));
        }

        // The scroll settles somewhere else entirely.
        q.set_visible(&[3]);

        assert_eq!(
            drain(&q),
            [3, 1, 2],
            "the tile on screen is served first, and the abandoned requests still beat the \
             background backlog"
        );
        for waiter in abandoned {
            assert!(waiter.join().unwrap());
        }
    }

    /// A deferred job is not handed to a worker before its time, but is neither dropped nor
    /// left for a worker to poll for: `pop_blocking` wakes on its own once the deadline
    /// passes, with no caller re-checking in a loop.
    #[test]
    fn defer_holds_a_job_back_until_its_time_and_then_serves_it() {
        let q = Arc::new(ThumbQueue::new());
        let not_before = Instant::now() + Duration::from_millis(80);
        q.defer(1, Priority::Background, not_before);
        assert!(q.is_empty(), "not eligible yet, so not counted as queued");

        let worker = {
            let q = q.clone();
            std::thread::spawn(move || q.pop_blocking())
        };
        assert_eq!(worker.join().unwrap(), Some(1));
        assert!(
            Instant::now() >= not_before,
            "served no earlier than its deadline"
        );
        q.done(1);
    }

    /// `wait_idle` must not declare the queue idle while a deferred job is still waiting out
    /// its backoff - that job is still outstanding work, even with nothing in `order` or
    /// `in_flight` right now.
    #[test]
    fn wait_idle_waits_out_a_deferred_job_too() {
        let q = Arc::new(ThumbQueue::new());
        q.defer(
            1,
            Priority::Background,
            Instant::now() + Duration::from_millis(80),
        );
        let waiter = {
            let q = q.clone();
            std::thread::spawn(move || q.wait_idle())
        };
        // A worker that never shows up to pop it - wait_idle must still return once the
        // deferred job's own deadline passes and it's admitted, then popped and finished by a
        // second thread standing in for a real worker.
        std::thread::sleep(Duration::from_millis(20));
        assert!(
            !waiter.is_finished(),
            "must not report idle while a deferred job is still pending"
        );
        let id = q.pop_blocking().unwrap();
        q.done(id);
        waiter.join().unwrap();
    }

    /// A push arriving while `id` is backing off (`defer`) must not readmit it before its
    /// deadline - that was exactly what let `request`'s own retries (and `set_visible`,
    /// `prioritize`, `enqueue_pending`) defeat `SUSPECT_BACKOFF_START`, popping the same
    /// suspect again within about a millisecond of it having just timed out. The raised
    /// priority still takes effect once the deadline arrives, and not a moment before.
    ///
    /// Probe: revert `State::push`'s delayed check (drop the `self.delayed.iter_mut().find`
    /// branch) - `q.is_empty()` then fails RED, because the plain push readmits id 1 into
    /// `order` immediately instead of leaving it delayed.
    #[test]
    fn a_push_of_a_delayed_id_raises_its_priority_without_readmitting_it_early() {
        let q = Arc::new(ThumbQueue::new());
        let not_before = Instant::now() + Duration::from_millis(80);
        q.defer(1, Priority::Background, not_before);

        q.push(1, Priority::Visible);
        assert!(
            q.is_empty(),
            "still backing off - a later push must not readmit it early"
        );
        assert_eq!(
            q.delayed_priority(1),
            Some(Priority::Visible),
            "but the priority it will run at is raised in place"
        );

        let worker = {
            let q = q.clone();
            std::thread::spawn(move || q.pop_blocking())
        };
        assert_eq!(worker.join().unwrap(), Some(1));
        assert!(
            Instant::now() >= not_before,
            "served no earlier than its deadline"
        );
        q.done(1);
    }

    /// A suspect backing off keeps the priority it will run at in `delayed`, not in `order`,
    /// so scrolling its tile away has to lower it there too - or it comes back from its
    /// back-off at `Visible` and goes ahead of the tiles actually on screen.
    #[test]
    fn scrolling_away_demotes_a_delayed_id_too() {
        let q = ThumbQueue::new();
        q.set_visible(&[1]);
        let id = q.pop_blocking().unwrap();
        q.defer(
            id,
            Priority::Visible,
            Instant::now() + Duration::from_secs(5),
        );
        q.done(id);

        q.set_visible(&[2]);
        assert_eq!(q.delayed_priority(1), Some(Priority::Background));
    }

    /// A push that arrives while its id is in flight waits in `deferred` and is queued at
    /// that priority once the job finishes. Scrolling away has to lower it there too, or the
    /// photo is queued `Visible` ahead of the tiles actually on screen.
    #[test]
    fn scrolling_away_demotes_a_push_waiting_on_its_running_job() {
        let q = ThumbQueue::new();
        q.push(1, Priority::Background);
        let id = q.pop_blocking().unwrap();
        q.push(9, Priority::Background);
        q.set_visible(&[1]);
        q.set_visible(&[2]);
        q.done(id);
        assert_eq!(drain(&q), [2, 9, 1]);
    }

    /// The waiter floor holds there too: an id someone is waiting on is not dropped behind the
    /// whole backlog just because its push arrived while it was running.
    #[test]
    fn a_waited_on_push_waiting_on_its_running_job_is_demoted_no_further_than_neighbour() {
        let q = Arc::new(ThumbQueue::new());
        q.push(1, Priority::Background);
        let id = q.pop_blocking().unwrap();
        q.push(9, Priority::Background);
        q.set_visible(&[1]);
        let waiter = {
            let q = q.clone();
            std::thread::spawn(move || q.wait_for(1, Instant::now() + Duration::from_secs(5)))
        };
        while !q.has_waiter(1) {
            std::thread::yield_now();
        }
        q.set_visible(&[2]);
        q.done(id);
        assert_eq!(drain(&q), [2, 1, 9]);
        assert!(waiter.join().unwrap());
    }

    /// `forget` clears a pending backoff outright - used once an id's fate no longer depends
    /// on winning `decode_lock` again, so a stale delayed entry doesn't sit around waking a
    /// worker for a job that has already been decided some other way. A no-op for anything
    /// reached through the queue itself (`admit_due` already emptied `delayed` of it before
    /// `pop` handed it out) - `ThumbService::get_or_generate` bypassing the queue entirely
    /// is the only way this situation arises today, and it's test-only (see its own doc).
    #[test]
    fn forget_drops_a_pending_backoff_with_no_replacement() {
        let q = ThumbQueue::new();
        q.defer(
            1,
            Priority::Background,
            Instant::now() + Duration::from_secs(5),
        );
        q.forget(1);
        assert!(q.delayed_priority(1).is_none());
        assert!(q.is_empty(), "not admitted either - just gone");
    }

    #[test]
    fn demotion_works_again_once_the_waiter_has_left() {
        let q = Arc::new(ThumbQueue::new());
        q.push(9, Priority::Background);
        q.set_visible(&[1]);
        let waiter = {
            let q = q.clone();
            std::thread::spawn(move || q.wait_for(1, Instant::now() + Duration::from_millis(50)))
        };
        assert!(
            !waiter.join().unwrap(),
            "the waiter times out while 1 is queued"
        );
        assert!(!q.has_waiter(1));

        q.set_visible(&[2]);
        assert_eq!(drain(&q), [2, 9, 1]);
    }

    /// A waker that counts its wakes.
    #[derive(Default)]
    struct Wakes(AtomicUsize);

    impl Wake for Wakes {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl Wakes {
        fn count(&self) -> usize {
            self.0.load(Ordering::SeqCst)
        }
    }

    fn poll(wait: &mut WaitFor<'_>, wakes: &Arc<Wakes>) -> Poll<()> {
        let waker = Waker::from(wakes.clone());
        Pin::new(wait).poll(&mut Context::from_waker(&waker))
    }

    /// Each finished job wakes the requests waiting on it and no others: during a fast
    /// scroll hundreds of requests are parked at once, and a broadcast would have every one
    /// of them re-check on every job any worker finishes.
    #[test]
    fn a_finished_job_wakes_only_the_waits_on_its_own_id() {
        let q = ThumbQueue::new();
        q.push_many(&[1, 2], Priority::Visible);
        assert_eq!((q.pop_blocking(), q.pop_blocking()), (Some(1), Some(2)));
        let (one, two) = (Arc::new(Wakes::default()), Arc::new(Wakes::default()));
        let mut wait_one = q.wait(1);
        let mut wait_two = q.wait(2);
        assert_eq!(poll(&mut wait_one, &one), Poll::Pending);
        assert_eq!(poll(&mut wait_two, &two), Poll::Pending);

        q.done(2);
        assert_eq!((one.count(), two.count()), (0, 1));
        assert_eq!(poll(&mut wait_two, &two), Poll::Ready(()));

        q.done_settled(1);
        assert_eq!(one.count(), 1);
        assert_eq!(poll(&mut wait_one, &one), Poll::Ready(()));
    }

    /// A future may be polled any number of times before it is woken; it holds one place,
    /// not one per poll, or a parked request's wakers would pile up for the whole timeout.
    #[test]
    fn a_wait_polled_again_replaces_its_waker() {
        let q = ThumbQueue::new();
        q.push(1, Priority::Visible);
        let mut wait = q.wait(1);
        let wakes = Arc::new(Wakes::default());
        for _ in 0..3 {
            assert_eq!(poll(&mut wait, &wakes), Poll::Pending);
        }
        assert_eq!(q.parked_wakers(), 1);
        assert_eq!(q.pop_blocking(), Some(1));
        q.done(1);
        assert_eq!(wakes.count(), 1);
    }

    /// Dropping a wait - a request timing out, or abandoned - takes its waker and its
    /// `Neighbour` floor with it.
    #[test]
    fn a_dropped_wait_leaves_nothing_behind() {
        let q = ThumbQueue::new();
        q.push(1, Priority::Visible);
        let mut wait = q.wait(1);
        assert_eq!(poll(&mut wait, &Arc::new(Wakes::default())), Poll::Pending);
        assert!(q.has_waiter(1));
        drop(wait);
        assert!(!q.has_waiter(1));
        assert_eq!(q.parked_wakers(), 0);
    }

    #[test]
    fn close_wakes_every_wait() {
        let q = ThumbQueue::new();
        q.push_many(&[1, 2], Priority::Visible);
        let wakes = Arc::new(Wakes::default());
        let mut waits = [q.wait(1), q.wait(2)];
        for wait in &mut waits {
            assert_eq!(poll(wait, &wakes), Poll::Pending);
        }
        q.close();
        assert_eq!(wakes.count(), 2);
        for wait in &mut waits {
            assert_eq!(poll(wait, &wakes), Poll::Ready(()));
        }
    }
}
