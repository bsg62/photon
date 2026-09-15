use parking_lot::{Condvar, Mutex};
use std::collections::{BTreeSet, HashMap, HashSet};

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
    /// Number of live `wait_for` callers per id. An id someone is waiting on is demoted no
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
    closed: bool,
}

impl State {
    fn push(&mut self, id: i64, priority: Priority) {
        if self.in_flight.contains(&id) {
            let held = self.deferred.entry(id).or_insert(priority);
            *held = (*held).min(priority);
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
    }

    fn done(&mut self, id: i64) {
        self.in_flight.remove(&id);
        if let Some(priority) = self.deferred.remove(&id) {
            self.push(id, priority);
        }
    }

    fn pop(&mut self) -> Option<i64> {
        let (_, _, id) = self.order.pop_first()?;
        self.entries.remove(&id);
        Some(id)
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
    pub fn pop_blocking(&self) -> Option<i64> {
        let mut state = self.state.lock();
        loop {
            if state.closed {
                return None;
            }
            if let Some(id) = state.pop() {
                state.in_flight.insert(id);
                return Some(id);
            }
            self.changed.wait(&mut state);
        }
    }

    /// Marks the popped job `id` finished and wakes idle-waiters and `wait_for` callers.
    pub fn done(&self, id: i64) {
        self.state.lock().done(id);
        self.changed.notify_all();
    }

    pub fn wait_idle(&self) {
        let mut state = self.state.lock();
        while !state.closed && !(state.order.is_empty() && state.in_flight.is_empty()) {
            self.changed.wait(&mut state);
        }
    }

    /// Blocks until `id` is neither queued nor being processed, the queue closes, or
    /// `deadline` passes. Returns false only on timeout.
    ///
    /// While waiting, `id` cannot be demoted past `Neighbour` (see `State::waiters`); the
    /// protection is dropped on every exit path, including the timeout, so a later
    /// `set_visible` demotes the id normally once nobody is waiting for it.
    pub fn wait_for(&self, id: i64, deadline: std::time::Instant) -> bool {
        let mut state = self.state.lock();
        *state.waiters.entry(id).or_insert(0) += 1;
        let done = loop {
            let busy = state.entries.contains_key(&id) || state.in_flight.contains(&id);
            if state.closed || !busy {
                break true;
            }
            if self.changed.wait_until(&mut state, deadline).timed_out() {
                let busy = state.entries.contains_key(&id) || state.in_flight.contains(&id);
                break !busy;
            }
        };
        if let std::collections::hash_map::Entry::Occupied(mut waiters) = state.waiters.entry(id) {
            *waiters.get_mut() -= 1;
            if *waiters.get() == 0 {
                waiters.remove();
            }
        }
        done
    }

    pub fn len(&self) -> usize {
        self.state.lock().order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn close(&self) {
        self.state.lock().closed = true;
        self.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    impl ThumbQueue {
        fn has_waiter(&self, id: i64) -> bool {
            self.state.lock().waiters.contains_key(&id)
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
}
