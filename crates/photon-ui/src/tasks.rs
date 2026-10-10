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
        mpsc::{self, Receiver},
    },
    thread,
};

/// The work panicked. Its own answer fails; the worker goes on to the next question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panicked;

struct Slot<J> {
    pending: Option<(u64, J)>,
    closed: bool,
}

struct Shared<J> {
    slot: Mutex<Slot<J>>,
    wake: Condvar,
}

pub struct Latest<J, R> {
    shared: Arc<Shared<J>>,
    answers: Receiver<(u64, Result<R, Panicked>)>,
    /// The latest question's number. An answer carrying an older one is dropped.
    asked: u64,
    /// The number of the last answer handed out.
    answered: u64,
}

impl<J: Send + 'static, R: Send + 'static> Latest<J, R> {
    /// Starts the worker. `work` answers one question; `notify` is called after each answer
    /// is ready, from the worker's thread, so the UI can ask to be drawn again.
    pub fn spawn(
        name: &str,
        work: impl Fn(J) -> R + Send + 'static,
        notify: impl Fn() + Send + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            slot: Mutex::new(Slot {
                pending: None,
                closed: false,
            }),
            wake: Condvar::new(),
        });
        let (tx, answers) = mpsc::channel();
        let theirs = shared.clone();
        thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                loop {
                    let (number, job) = {
                        let mut slot = theirs.slot.lock();
                        loop {
                            if slot.closed {
                                return;
                            }
                            if let Some(pending) = slot.pending.take() {
                                break pending;
                            }
                            theirs.wake.wait(&mut slot);
                        }
                    };
                    let answer = catch_unwind(AssertUnwindSafe(|| work(job))).map_err(|_| Panicked);
                    if tx.send((number, answer)).is_err() {
                        return;
                    }
                    notify();
                }
            })
            .expect("a worker thread could not be started");
        Self {
            shared,
            answers,
            asked: 0,
            answered: 0,
        }
    }

    /// Asks. A question still waiting for the worker is replaced; one being worked on
    /// finishes, and its answer is dropped.
    pub fn ask(&mut self, job: J) {
        self.asked += 1;
        self.shared.slot.lock().pending = Some((self.asked, job));
        self.shared.wake.notify_one();
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
}

/// The worker ends at its next wait. It is not joined: work in flight may be a database
/// read on a slow disk, and closing the window must not wait for it.
impl<J, R> Drop for Latest<J, R> {
    fn drop(&mut self) {
        self.shared.slot.lock().closed = true;
        self.shared.wake.notify_all();
    }
}

struct Line<J> {
    /// The jobs not yet started, oldest first, each with its number.
    waiting: VecDeque<(u64, J)>,
    closed: bool,
}

struct SharedLine<J> {
    line: Mutex<Line<J>>,
    wake: Condvar,
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
    answers: Receiver<(u64, Result<R, Panicked>)>,
    given: u64,
}

impl<J: Send + 'static, R: Send + 'static> Queue<J, R> {
    /// Starts the worker. `work` does one job; `notify` is called after each answer is
    /// ready, from the worker's thread, so the UI can ask to be drawn again.
    pub fn spawn(
        name: &str,
        work: impl Fn(J) -> R + Send + 'static,
        notify: impl Fn() + Send + 'static,
    ) -> Self {
        let shared = Arc::new(SharedLine {
            line: Mutex::new(Line {
                waiting: VecDeque::new(),
                closed: false,
            }),
            wake: Condvar::new(),
        });
        let (tx, answers) = mpsc::channel();
        let theirs = shared.clone();
        thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                loop {
                    let (number, job) = {
                        let mut line = theirs.line.lock();
                        loop {
                            if line.closed {
                                return;
                            }
                            if let Some(next) = line.waiting.pop_front() {
                                break next;
                            }
                            theirs.wake.wait(&mut line);
                        }
                    };
                    // A job that panics fails its own answer and no other: a queue that
                    // ended here would leave every later click unanswered for the session.
                    let answer = catch_unwind(AssertUnwindSafe(|| work(job))).map_err(|_| Panicked);
                    if tx.send((number, answer)).is_err() {
                        return;
                    }
                    notify();
                }
            })
            .expect("a worker thread could not be started");
        Self {
            shared,
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
        let mut line = self.shared.line.lock();
        if let Some((number, last)) = line.waiting.back_mut()
            && replaces(last)
        {
            *last = job;
            return *number;
        }
        self.given += 1;
        line.waiting.push_back((self.given, job));
        drop(line);
        self.shared.wake.notify_one();
        self.given
    }

    /// The answers that have come since this was last asked, oldest first, each with its
    /// job's number.
    pub fn answers(&mut self) -> Vec<(u64, Result<R, Panicked>)> {
        self.answers.try_iter().collect()
    }
}

/// As `Latest`: the worker ends at its next wait and is not joined. Jobs still waiting
/// are not done: the window is closing. For the views' queue each is a rebuild nobody
/// will see. For a queue of writes it is something the user asked for in the instant
/// before closing the window, and it is lost: whoever puts a write here that must not
/// be has to wait for the queue before the window goes.
impl<J, R> Drop for Queue<J, R> {
    fn drop(&mut self) {
        let mut line = self.shared.line.lock();
        line.closed = true;
        line.waiting.clear();
        drop(line);
        self.shared.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::mpsc::{Sender, channel},
        time::{Duration, Instant},
    };

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
}
