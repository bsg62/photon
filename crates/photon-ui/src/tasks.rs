//! Work the UI thread must not do itself: anything that reads SQLite or the filesystem.
//!
//! One `Latest` per kind of question. It keeps one pending question - a newer one replaces
//! it, since its answer is no longer wanted - and hands back only the answer to the latest
//! question asked. This is the job `LibraryStore`'s generation counters did in the Svelte
//! UI, where a late answer otherwise overwrote a newer one.
//!
//! And one `Queue` for what must not be dropped and must not be reordered: the steps that
//! move the engine's view. That is the Svelte store's `viewChain`.

use parking_lot::{Condvar, Mutex};
use std::{
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

/// The work panicked. Its own answer fails; the worker goes on to the next question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panicked;

struct PoolState {
    jobs: VecDeque<Box<dyn FnOnce() + Send>>,
    closed: bool,
}

struct PoolShared {
    state: Mutex<PoolState>,
    wake: Condvar,
}

/// Closes the pool's workers once the pool handle and every `Latest`/`Queue` backed by it
/// have been dropped (workers hold `Arc<PoolShared>` directly, never `PoolDrop`).
struct PoolDrop {
    shared: Arc<PoolShared>,
}

impl PoolDrop {
    fn enqueue(&self, job: Box<dyn FnOnce() + Send>) {
        let mut state = self.shared.state.lock();
        if state.closed {
            return;
        }
        state.jobs.push_back(job);
        drop(state);
        self.shared.wake.notify_one();
    }
}

impl Drop for PoolDrop {
    fn drop(&mut self) {
        let mut state = self.shared.state.lock();
        state.closed = true;
        state.jobs.clear();
        drop(state);
        self.shared.wake.notify_all();
    }
}

/// A bounded pool of worker threads shared by [`Latest`] and [`Queue`] tasks.
///
/// Each [`Latest`] or [`Queue`] created on a pool keeps its own ordering and coalescing
/// guarantees (at most one job in flight per task handle), while sharing the pool's OS
/// threads instead of spawning a permanent thread for every UI query.
#[derive(Clone)]
pub struct Pool {
    inner: Arc<PoolDrop>,
}

impl Pool {
    /// Starts `workers` threads named `{name}-{i}` (or `{name}` when `workers == 1`).
    pub fn spawn(name: &str, workers: usize) -> Self {
        let workers = workers.max(1);
        let shared = Arc::new(PoolShared {
            state: Mutex::new(PoolState {
                jobs: VecDeque::new(),
                closed: false,
            }),
            wake: Condvar::new(),
        });
        for i in 0..workers {
            let theirs = shared.clone();
            let thread_name = if workers == 1 {
                name.to_owned()
            } else {
                format!("{name}-{i}")
            };
            thread::Builder::new()
                .name(thread_name)
                .spawn(move || {
                    loop {
                        let job = {
                            let mut state = theirs.state.lock();
                            loop {
                                if state.closed {
                                    return;
                                }
                                if let Some(job) = state.jobs.pop_front() {
                                    break job;
                                }
                                theirs.wake.wait(&mut state);
                            }
                        };
                        job();
                    }
                })
                .expect("a worker thread could not be started");
        }
        Self {
            inner: Arc::new(PoolDrop { shared }),
        }
    }

    /// Creates a [`Latest`] task backed by this pool.
    pub fn latest<J: Send + 'static, R: Send + 'static>(
        &self,
        work: impl Fn(J) -> R + Send + 'static,
        notify: impl Fn() + Send + 'static,
    ) -> Latest<J, R> {
        Latest::on_pool(self.inner.clone(), work, notify)
    }

    /// Creates a [`Queue`] task backed by this pool.
    pub fn queue<J: Send + 'static, R: Send + 'static>(
        &self,
        work: impl Fn(J) -> R + Send + 'static,
        notify: impl Fn() + Send + 'static,
    ) -> Queue<J, R> {
        Queue::on_pool(self.inner.clone(), work, notify)
    }
}

struct Slot<J> {
    pending: Option<(u64, J)>,
    scheduled: bool,
    closed: bool,
}

struct Runner<F, N> {
    work: F,
    notify: N,
}

struct LatestTask<J, R, F, N> {
    slot: Arc<Mutex<Slot<J>>>,
    runner: Mutex<Runner<F, N>>,
    tx: Sender<(u64, Result<R, Panicked>)>,
    pool: Arc<PoolDrop>,
}

fn schedule_latest<J, R, F, N>(task: Arc<LatestTask<J, R, F, N>>)
where
    J: Send + 'static,
    R: Send + 'static,
    F: Fn(J) -> R + Send + 'static,
    N: Fn() + Send + 'static,
{
    let pool = task.pool.clone();
    pool.enqueue(Box::new(move || {
        let (number, job) = {
            let mut slot = task.slot.lock();
            if slot.closed {
                slot.scheduled = false;
                return;
            }
            match slot.pending.take() {
                Some(pending) => pending,
                None => {
                    slot.scheduled = false;
                    return;
                }
            }
        };
        let answer = {
            let runner = task.runner.lock();
            catch_unwind(AssertUnwindSafe(|| (runner.work)(job))).map_err(|_| Panicked)
        };
        let sent = task.tx.send((number, answer)).is_ok();
        if sent {
            let runner = task.runner.lock();
            (runner.notify)();
        }
        let reschedule = {
            let mut slot = task.slot.lock();
            if !sent || slot.closed || slot.pending.is_none() {
                slot.scheduled = false;
                false
            } else {
                true
            }
        };
        if reschedule {
            schedule_latest(task);
        }
    }));
}

pub struct Latest<J, R> {
    slot: Arc<Mutex<Slot<J>>>,
    schedule: Box<dyn Fn() + Send>,
    answers: Receiver<(u64, Result<R, Panicked>)>,
    /// The latest question's number. An answer carrying an older one is dropped.
    asked: u64,
    /// The number of the last answer handed out.
    answered: u64,
}

impl<J: Send + 'static, R: Send + 'static> Latest<J, R> {
    /// Starts a dedicated worker thread for this task. Prefer [`Pool::latest`] when many
    /// short tasks can share threads.
    pub fn spawn(
        name: &str,
        work: impl Fn(J) -> R + Send + 'static,
        notify: impl Fn() + Send + 'static,
    ) -> Self {
        Pool::spawn(name, 1).latest(work, notify)
    }

    fn on_pool(
        pool: Arc<PoolDrop>,
        work: impl Fn(J) -> R + Send + 'static,
        notify: impl Fn() + Send + 'static,
    ) -> Self {
        let slot = Arc::new(Mutex::new(Slot {
            pending: None,
            scheduled: false,
            closed: false,
        }));
        let (tx, answers) = mpsc::channel();
        let task = Arc::new(LatestTask {
            slot: slot.clone(),
            runner: Mutex::new(Runner { work, notify }),
            tx,
            pool,
        });
        let schedule = Box::new(move || schedule_latest(task.clone()));
        Self {
            slot,
            schedule,
            answers,
            asked: 0,
            answered: 0,
        }
    }

    /// Asks. A question still waiting for the worker is replaced; one being worked on
    /// finishes, and its answer is dropped.
    pub fn ask(&mut self, job: J) {
        self.asked += 1;
        let should_schedule = {
            let mut slot = self.slot.lock();
            slot.pending = Some((self.asked, job));
            if slot.scheduled {
                false
            } else {
                slot.scheduled = true;
                true
            }
        };
        if should_schedule {
            (self.schedule)();
        }
    }

    /// The answer to the latest question, once it is there, once.
    pub fn answer(&mut self) -> Option<Result<R, Panicked>> {
        let mut latest = None;
        while let Ok((number, answer)) = self.answers.try_recv() {
            if number == self.asked {
                self.answered = number;
                latest = Some(answer);
            }
        }
        latest
    }

    /// Whether a question has been asked whose answer has not been handed out.
    pub fn waiting(&self) -> bool {
        self.answered < self.asked
    }

    /// Waits until the latest question has been answered, for `patience` at most, and
    /// says whether it was. For a `Latest` that writes, as the window closes: the sidebar's
    /// width dragged, or the folder scrolled to, in the instant before was otherwise lost
    /// with the task. The answer is taken here and not handed out.
    pub fn finish(&mut self, patience: Duration) -> bool {
        let until = Instant::now() + patience;
        while self.waiting() {
            let left = until.saturating_duration_since(Instant::now());
            match self.answers.recv_timeout(left) {
                Ok((number, _)) if number == self.asked => self.answered = number,
                Ok(_) => {}
                Err(_) => return false,
            }
        }
        true
    }
}

/// The worker stops picking up further questions from this handle. Work already in flight
/// is not joined: it may be a database read on a slow disk, and closing the window must
/// not wait for it.
impl<J, R> Drop for Latest<J, R> {
    fn drop(&mut self) {
        let mut slot = self.slot.lock();
        slot.closed = true;
        slot.pending = None;
    }
}

struct Line<J> {
    /// The jobs not yet started, oldest first, each with its number.
    waiting: VecDeque<(u64, J)>,
    /// Whether a job is being done.
    working: bool,
    /// Whether a worker task is queued or running for this queue.
    scheduled: bool,
    closed: bool,
}

struct SharedLine<J> {
    line: Mutex<Line<J>>,
    /// Told when a job is done, for whoever waits for the queue to be empty.
    done: Condvar,
}

struct QueueTask<J, R, F, N> {
    shared: Arc<SharedLine<J>>,
    runner: Mutex<Runner<F, N>>,
    tx: Sender<(u64, Result<R, Panicked>)>,
    pool: Arc<PoolDrop>,
}

fn schedule_queue<J, R, F, N>(task: Arc<QueueTask<J, R, F, N>>)
where
    J: Send + 'static,
    R: Send + 'static,
    F: Fn(J) -> R + Send + 'static,
    N: Fn() + Send + 'static,
{
    let pool = task.pool.clone();
    pool.enqueue(Box::new(move || {
        let (number, job) = {
            let mut line = task.shared.line.lock();
            if line.closed {
                line.scheduled = false;
                return;
            }
            match line.waiting.pop_front() {
                Some(next) => {
                    line.working = true;
                    next
                }
                None => {
                    line.scheduled = false;
                    return;
                }
            }
        };
        // A job that panics fails its own answer and no other: a queue that
        // ended here would leave every later click unanswered for the session.
        let answer = {
            let runner = task.runner.lock();
            catch_unwind(AssertUnwindSafe(|| (runner.work)(job))).map_err(|_| Panicked)
        };
        // The answer first, then the word that the job is done: whoever
        // waited for the queue finds every answer there.
        let sent = task.tx.send((number, answer)).is_ok();
        let reschedule = {
            let mut line = task.shared.line.lock();
            line.working = false;
            task.shared.done.notify_all();
            if !sent || line.closed || line.waiting.is_empty() {
                line.scheduled = false;
                false
            } else {
                true
            }
        };
        if sent {
            let runner = task.runner.lock();
            (runner.notify)();
        }
        if reschedule {
            schedule_queue(task);
        }
    }));
}

/// Every job, in the order it was given.
///
/// A step that moves the engine's view rebuilds the grid on the thread that makes it, so
/// it is made here; and a step already begun cannot be taken back, so the one after it
/// waits. `Latest` keeps one waiting question and puts a newer one in its place: a sort
/// asked for and then a view clicked while a rebuild was running, and the sort would
/// never be made.
pub struct Queue<J, R> {
    shared: Arc<SharedLine<J>>,
    schedule: Box<dyn Fn() + Send>,
    answers: Receiver<(u64, Result<R, Panicked>)>,
    given: u64,
}

impl<J: Send + 'static, R: Send + 'static> Queue<J, R> {
    /// Starts a dedicated worker thread for this queue. Prefer [`Pool::queue`] when
    /// multiple queues can share a worker pool.
    pub fn spawn(
        name: &str,
        work: impl Fn(J) -> R + Send + 'static,
        notify: impl Fn() + Send + 'static,
    ) -> Self {
        Pool::spawn(name, 1).queue(work, notify)
    }

    fn on_pool(
        pool: Arc<PoolDrop>,
        work: impl Fn(J) -> R + Send + 'static,
        notify: impl Fn() + Send + 'static,
    ) -> Self {
        let shared = Arc::new(SharedLine {
            line: Mutex::new(Line {
                waiting: VecDeque::new(),
                working: false,
                scheduled: false,
                closed: false,
            }),
            done: Condvar::new(),
        });
        let (tx, answers) = mpsc::channel();
        let task = Arc::new(QueueTask {
            shared: shared.clone(),
            runner: Mutex::new(Runner { work, notify }),
            tx,
            pool,
        });
        let schedule = Box::new(move || schedule_queue(task.clone()));
        Self {
            shared,
            schedule,
            answers,
            given: 0,
        }
    }

    /// Adds `job` to the end, and answers its number.
    pub fn push(&mut self, job: J) -> u64 {
        self.push_or_replace(job, |_| false)
    }

    /// Adds `job` to the end - or puts it in the place of the last job, keeping that job's
    /// number, when that one has not started and `replaces` says it may go. Only the last:
    /// with another job behind it, replacing it would have the two swap their order.
    pub fn push_or_replace(&mut self, job: J, replaces: impl FnOnce(&J) -> bool) -> u64 {
        let (number, should_schedule) = {
            let mut line = self.shared.line.lock();
            if let Some((number, last)) = line.waiting.back_mut()
                && replaces(last)
            {
                *last = job;
                return *number;
            }
            self.given += 1;
            line.waiting.push_back((self.given, job));
            let schedule = if line.scheduled {
                false
            } else {
                line.scheduled = true;
                true
            };
            (self.given, schedule)
        };
        if should_schedule {
            (self.schedule)();
        }
        number
    }

    /// The answers that have come since this was last asked, oldest first, each with its
    /// job's number.
    pub fn answers(&mut self) -> Vec<(u64, Result<R, Panicked>)> {
        self.answers.try_iter().collect()
    }

    /// Waits until every job given has been done, for `patience` at most, and says
    /// whether they all were. For a queue of writes as the window closes: dropped
    /// there, a star pressed in the instant before was lost without a word. Not for
    /// ever, because a write can hang - a folder on a share that has gone - and a
    /// window that cannot be closed is worse than a star not written.
    pub fn finish(&mut self, patience: Duration) -> bool {
        let until = Instant::now() + patience;
        let mut line = self.shared.line.lock();
        while line.working || !line.waiting.is_empty() {
            if self.shared.done.wait_until(&mut line, until).timed_out() {
                return false;
            }
        }
        true
    }
}

/// As `Latest`: work in flight is not joined. Jobs still waiting are not done: the window
/// is closing. For the views' queue each is a rebuild nobody will see. For a queue of
/// writes it is something the user asked for in the instant before closing the window, and
/// it is lost: whoever puts a write here that must not be waits for the queue before the
/// window goes (`finish`).
impl<J, R> Drop for Queue<J, R> {
    fn drop(&mut self) {
        let mut line = self.shared.line.lock();
        line.closed = true;
        line.waiting.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::mpsc::{Sender, channel},
        time::{Duration, Instant},
    };

    #[test]
    fn finish_waits_for_the_latest_question_and_no_longer_than_it_is_told() {
        let (done, told) = channel();
        let mut task: Latest<u32, ()> = Latest::spawn(
            "finish",
            move |job| {
                std::thread::sleep(Duration::from_millis(u64::from(job)));
                done.send(job).unwrap();
            },
            || {},
        );
        task.ask(150);
        assert!(task.finish(Duration::from_secs(10)));
        assert_eq!(told.try_recv(), Ok(150), "answered before it was done");
        assert!(!task.waiting());
        // Nothing asked since: nothing to wait for.
        assert!(task.finish(Duration::ZERO));
        task.ask(2_000);
        assert!(!task.finish(Duration::from_millis(50)));
    }

    /// Waits for an answer; a worker that never answers fails the test instead of hanging it.
    fn answer_of<J: Send + 'static, R: Send + 'static>(
        task: &mut Latest<J, R>,
    ) -> Result<R, Panicked> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(answer) = task.answer() {
                return answer;
            }
            assert!(Instant::now() < deadline, "no answer within ten seconds");
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// A worker that says when it has started a job and then waits to be let go.
    fn gated() -> (Latest<u32, u32>, Receiver<u32>, Sender<()>) {
        let (started_tx, started) = channel();
        let (go, gate) = channel::<()>();
        let task = Latest::spawn(
            "test",
            move |job: u32| {
                started_tx.send(job).unwrap();
                gate.recv().unwrap();
                job * 10
            },
            || {},
        );
        (task, started, go)
    }

    #[test]
    fn a_question_is_answered_once() {
        let mut task = Latest::spawn("test", |job: u32| job + 1, || {});
        assert!(!task.waiting());
        task.ask(1);
        assert!(task.waiting());
        assert_eq!(answer_of(&mut task), Ok(2));
        assert!(!task.waiting());
        assert_eq!(task.answer(), None);
    }

    #[test]
    fn a_newer_question_replaces_the_one_still_waiting() {
        let (mut task, started, go) = gated();
        task.ask(1);
        assert_eq!(started.recv().unwrap(), 1);
        // The worker is busy with 1: 2 waits, and 3 takes its place.
        task.ask(2);
        task.ask(3);
        go.send(()).unwrap();
        assert_eq!(started.recv().unwrap(), 3, "2 was never started");
        go.send(()).unwrap();
        assert_eq!(answer_of(&mut task), Ok(30));
    }

    #[test]
    fn an_answer_to_an_older_question_is_dropped() {
        let (mut task, started, go) = gated();
        task.ask(1);
        assert_eq!(started.recv().unwrap(), 1);
        task.ask(2);
        // 1 finishes after 2 was asked: its answer must never be handed out.
        go.send(()).unwrap();
        assert_eq!(started.recv().unwrap(), 2);
        assert_eq!(task.answer(), None);
        assert!(task.waiting());
        go.send(()).unwrap();
        assert_eq!(answer_of(&mut task), Ok(20));
    }

    #[test]
    fn a_job_that_panics_fails_its_own_answer_and_the_next_one_runs() {
        let mut task = Latest::spawn(
            "test",
            |job: u32| {
                assert!(job != 1, "the job this test breaks on purpose");
                job
            },
            || {},
        );
        task.ask(1);
        assert_eq!(answer_of(&mut task), Err(Panicked));
        task.ask(2);
        assert_eq!(answer_of(&mut task), Ok(2));
    }

    #[test]
    fn the_ui_is_told_when_an_answer_is_ready() {
        let (told_tx, told) = channel();
        let mut task = Latest::spawn("test", |job: u32| job, move || told_tx.send(()).unwrap());
        task.ask(7);
        told.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(task.answer(), Some(Ok(7)));
    }

    /// Waits for `count` answers of a queue.
    fn answers_of<J: Send + 'static, R: Send + 'static>(
        queue: &mut Queue<J, R>,
        count: usize,
    ) -> Vec<(u64, Result<R, Panicked>)> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut all = Vec::new();
        while all.len() < count {
            all.extend(queue.answers());
            assert!(Instant::now() < deadline, "no answer within ten seconds");
            thread::sleep(Duration::from_millis(2));
        }
        all
    }

    /// A queue whose worker says when it has started a job and then waits to be let go.
    fn gated_queue() -> (Queue<u32, u32>, Receiver<u32>, Sender<()>) {
        let (started_tx, started) = channel();
        let (go, gate) = channel::<()>();
        let queue = Queue::spawn(
            "test",
            move |job: u32| {
                started_tx.send(job).unwrap();
                gate.recv().unwrap();
                job * 10
            },
            || {},
        );
        (queue, started, go)
    }

    // The difference from `Latest`: three steps asked while the first is being made are
    // all made, in the order asked.
    #[test]
    fn every_job_is_done_in_the_order_given() {
        let (mut queue, started, go) = gated_queue();
        assert_eq!(queue.push(1), 1);
        assert_eq!(started.recv().unwrap(), 1);
        assert_eq!(queue.push(2), 2);
        assert_eq!(queue.push(3), 3);
        for expected in [2, 3] {
            go.send(()).unwrap();
            assert_eq!(started.recv().unwrap(), expected);
        }
        go.send(()).unwrap();
        assert_eq!(
            answers_of(&mut queue, 3),
            [(1, Ok(10)), (2, Ok(20)), (3, Ok(30))]
        );
    }

    // A search typed further before it was sent: the later text goes in its place, under
    // its number, and the rebuild nobody wants is never made.
    #[test]
    fn the_last_job_is_replaced_while_it_has_not_started() {
        let (mut queue, started, go) = gated_queue();
        queue.push(1);
        assert_eq!(started.recv().unwrap(), 1);
        assert_eq!(queue.push(2), 2);
        assert_eq!(queue.push_or_replace(5, |last| *last == 2), 2);
        go.send(()).unwrap();
        assert_eq!(started.recv().unwrap(), 5, "2 was never started");
        go.send(()).unwrap();
        assert_eq!(answers_of(&mut queue, 2), [(1, Ok(10)), (2, Ok(50))]);
    }

    #[test]
    fn a_job_is_not_replaced_once_it_has_started_or_when_it_may_not_be() {
        let (mut queue, started, go) = gated_queue();
        queue.push(1);
        assert_eq!(started.recv().unwrap(), 1);
        // 1 has started: there is nothing waiting to replace.
        assert_eq!(queue.push_or_replace(2, |_| true), 2);
        // 2 is waiting, and may not go.
        assert_eq!(queue.push_or_replace(3, |_| false), 3);
        for expected in [2, 3] {
            go.send(()).unwrap();
            assert_eq!(started.recv().unwrap(), expected);
        }
        go.send(()).unwrap();
        assert_eq!(answers_of(&mut queue, 3).len(), 3);
    }

    // Only the last may be replaced. A search with a view switch asked behind it stays
    // where it is: put in the switch's place, or taken out, the two would land the other
    // way round.
    #[test]
    fn a_job_with_another_behind_it_is_not_replaced() {
        let (mut queue, started, go) = gated_queue();
        queue.push(1);
        assert_eq!(started.recv().unwrap(), 1);
        queue.push(2);
        queue.push(7);
        // "Replace a 2": the last job is the 7, so this is added.
        assert_eq!(queue.push_or_replace(9, |last| *last == 2), 4);
        for expected in [2, 7, 9] {
            go.send(()).unwrap();
            assert_eq!(started.recv().unwrap(), expected);
        }
        go.send(()).unwrap();
        assert_eq!(answers_of(&mut queue, 4).len(), 4);
    }

    #[test]
    fn a_queued_job_that_panics_fails_its_own_answer_and_the_next_one_runs() {
        let mut queue = Queue::spawn(
            "test",
            |job: u32| {
                assert!(job != 1, "the job this test breaks on purpose");
                job
            },
            || {},
        );
        queue.push(1);
        queue.push(2);
        assert_eq!(answers_of(&mut queue, 2), [(1, Err(Panicked)), (2, Ok(2))]);
    }

    #[test]
    fn the_ui_is_told_when_a_queued_job_is_done() {
        let (told_tx, told) = channel();
        let mut queue = Queue::spawn("test", |job: u32| job, move || told_tx.send(()).unwrap());
        queue.push(7);
        told.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(queue.answers(), [(1, Ok(7))]);
    }

    // A star pressed in the instant before the window is closed: the window waits for
    // it, where dropping the queue would lose it without a word.
    #[test]
    fn a_queue_that_is_finished_does_every_job_it_was_given_first() {
        // The worker rests a moment after each answer, before it takes its next job:
        // whoever waits for the queue is awake then, with a job still waiting and none
        // being done.
        let mut queue = Queue::spawn(
            "test",
            |job: u32| job * 10,
            || std::thread::sleep(Duration::from_millis(30)),
        );
        queue.push(1);
        queue.push(2);
        queue.push(3);
        assert!(queue.finish(Duration::from_secs(30)));
        assert_eq!(queue.answers(), [(1, Ok(10)), (2, Ok(20)), (3, Ok(30))]);
        // With nothing given, there is nothing to wait for.
        let mut idle = Queue::spawn("test", |job: u32| job, || {});
        assert!(idle.finish(Duration::from_secs(30)));
    }

    // The last job is no longer waiting: it is being done, and that is waited for too.
    #[test]
    fn a_job_being_done_is_waited_for_as_one_still_waiting_is() {
        let (mut queue, started, go) = gated_queue();
        queue.push(1);
        assert_eq!(started.recv().unwrap(), 1);
        assert!(!queue.finish(Duration::from_millis(50)));
        go.send(()).unwrap();
        assert!(queue.finish(Duration::from_secs(30)));
        assert_eq!(queue.answers(), [(1, Ok(10))]);
    }

    // A write that hangs - a folder on a share that has gone - must not keep the window
    // from closing for good.
    #[test]
    fn a_queue_is_waited_for_only_so_long() {
        let (mut queue, started, go) = gated_queue();
        queue.push(1);
        queue.push(2);
        assert_eq!(started.recv().unwrap(), 1);
        assert!(!queue.finish(Duration::from_millis(50)));
        // The job that had not begun is let go of with the queue: it is never started.
        drop(queue);
        go.send(()).unwrap();
        assert!(started.recv_timeout(Duration::from_millis(200)).is_err());
    }

    // Multiple `Latest` and `Queue` handles sharing one `Pool` preserve per-handle
    // coalescing and FIFO ordering without spawning a thread per handle.
    #[test]
    fn shared_pool_preserves_latest_and_queue_semantics() {
        let pool = Pool::spawn("shared", 2);
        let mut latest_a = pool.latest(|x: u32| x * 2, || {});
        let mut latest_b = pool.latest(|x: u32| x + 100, || {});
        let mut queue = pool.queue(|x: u32| x * 3, || {});
        latest_a.ask(4);
        latest_b.ask(5);
        queue.push(1);
        queue.push(2);
        queue.push(3);
        assert_eq!(answer_of(&mut latest_a), Ok(8));
        assert_eq!(answer_of(&mut latest_b), Ok(105));
        assert_eq!(
            answers_of(&mut queue, 3),
            [(1, Ok(3)), (2, Ok(6)), (3, Ok(9))]
        );
    }
}
