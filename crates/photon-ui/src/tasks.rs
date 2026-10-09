//! Work the UI thread must not do itself: anything that reads SQLite or the filesystem.
//!
//! One `Latest` per kind of question. It keeps one pending question - a newer one replaces
//! it, since its answer is no longer wanted - and hands back only the answer to the latest
//! question asked. This is the job `LibraryStore`'s generation counters did in the Svelte
//! UI, where a late answer otherwise overwrote a newer one.

use parking_lot::{Condvar, Mutex};
use std::{
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
}
