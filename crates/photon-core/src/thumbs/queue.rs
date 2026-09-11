use parking_lot::{Condvar, Mutex};
use std::collections::{BTreeSet, HashMap};

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
    active: usize,
    closed: bool,
}

impl State {
    fn push(&mut self, id: i64, priority: Priority) {
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
        if let Some(&(current, seq)) = self.entries.get(&id)
            && current < priority
        {
            self.order.remove(&(current, seq, id));
            self.order.insert((priority, seq, id));
            self.entries.insert(id, (priority, seq));
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

    /// Blocks until a job is available. Every `Some` must be followed by `done()`.
    pub fn pop_blocking(&self) -> Option<i64> {
        let mut state = self.state.lock();
        loop {
            if state.closed {
                return None;
            }
            if let Some(id) = state.pop() {
                state.active += 1;
                return Some(id);
            }
            self.changed.wait(&mut state);
        }
    }

    pub fn done(&self) {
        let mut state = self.state.lock();
        state.active = state.active.saturating_sub(1);
        drop(state);
        self.changed.notify_all();
    }

    pub fn wait_idle(&self) {
        let mut state = self.state.lock();
        while !state.closed && !(state.order.is_empty() && state.active == 0) {
            self.changed.wait(&mut state);
        }
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

    fn drain(q: &ThumbQueue) -> Vec<i64> {
        let mut out = Vec::new();
        while !q.is_empty() {
            out.push(q.pop_blocking().unwrap());
            q.done();
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
                q.done();
                id
            })
        };
        q.wait_idle();
        assert!(q.is_empty());
        assert_eq!(worker.join().unwrap(), 1);
    }
}
