//! A single bounded dispatcher; slow Start never runs in WM_COPYDATA.
use parking_lot::{Condvar, Mutex};
use std::collections::VecDeque;

pub(super) type Job = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct State {
    ready: bool,
    closed: bool,
    jobs: VecDeque<Job>,
}

#[derive(Default)]
pub(super) struct Queue {
    state: Mutex<State>,
    changed: Condvar,
}

impl Queue {
    pub fn push(&self, job: Job) -> Result<(), &'static str> {
        let mut state = self.state.lock();
        if state.closed {
            return Err("Fono завершается. Повторите команду после запуска.");
        }
        if state.jobs.len() >= 32 {
            return Err("Слишком много команд. Дождитесь завершения текущего действия.");
        }
        state.jobs.push_back(job);
        self.changed.notify_one();
        Ok(())
    }

    pub fn ready(&self) {
        self.state.lock().ready = true;
        self.changed.notify_one();
    }

    pub fn closed(&self) -> bool {
        self.state.lock().closed
    }

    pub fn run(&self) {
        loop {
            let job = {
                let mut state = self.state.lock();
                while !state.closed && (!state.ready || state.jobs.is_empty()) {
                    self.changed.wait(&mut state);
                }
                if state.closed {
                    return;
                }
                state.jobs.pop_front()
            };
            if let Some(job) = job {
                job();
            }
        }
    }

    pub fn close(&self) {
        let pending = {
            let mut state = self.state.lock();
            state.closed = true;
            std::mem::take(&mut state.jobs)
        };
        self.changed.notify_all();
        // Release admission guards after leaving the queue lock.
        drop(pending);
    }
}

#[cfg(test)]
mod tests;
