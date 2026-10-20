use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::Duration;

#[test]
fn commands_wait_for_runtime_ready_and_keep_fifo_order() {
    let queue = Arc::new(Queue::default());
    let (tx, rx) = mpsc::channel();
    for index in 0..3 {
        let tx = tx.clone();
        queue
            .push(Box::new(move || tx.send(index).unwrap()))
            .unwrap();
    }
    let worker_queue = queue.clone();
    let worker = std::thread::spawn(move || worker_queue.run());
    assert!(rx.recv_timeout(Duration::from_millis(30)).is_err());
    queue.ready();
    for index in 0..3 {
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), index);
    }
    queue.close();
    worker.join().unwrap();
}

struct Guard(Arc<AtomicBool>);
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[test]
fn abandoned_pending_jobs_release_their_actual_work_guard_and_never_run() {
    let queue = Queue::default();
    let dropped = Arc::new(AtomicBool::new(false));
    let ran = Arc::new(AtomicBool::new(false));
    let guard = Guard(dropped.clone());
    let job_ran = ran.clone();
    queue
        .push(Box::new(move || {
            let _guard = guard;
            job_ran.store(true, Ordering::Release);
        }))
        .unwrap();
    assert!(!dropped.load(Ordering::Acquire));
    queue.close();
    queue.ready();
    queue.run();
    assert!(dropped.load(Ordering::Acquire));
    assert!(!ran.load(Ordering::Acquire));
    assert!(queue.push(Box::new(|| panic!("closed queue ran"))).is_err());
}

#[test]
fn slow_start_keeps_guard_after_submission_and_does_not_block_enqueueing_stop() {
    let queue = Arc::new(Queue::default());
    let dropped = Arc::new(AtomicBool::new(false));
    let guard = Guard(dropped.clone());
    let (entered, started) = mpsc::channel();
    let (release, finish) = mpsc::channel();
    queue
        .push(Box::new(move || {
            let _guard = guard;
            entered.send(()).unwrap();
            finish.recv_timeout(Duration::from_secs(5)).unwrap();
        }))
        .unwrap();
    let worker_queue = queue.clone();
    let worker = std::thread::spawn(move || worker_queue.run());
    queue.ready();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let (stop, stopped) = mpsc::channel();
    queue
        .push(Box::new(move || stop.send(()).unwrap()))
        .unwrap();
    assert!(!dropped.load(Ordering::Acquire));
    release.send(()).unwrap();
    stopped.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(dropped.load(Ordering::Acquire));
    queue.close();
    worker.join().unwrap();
}

#[test]
fn queue_is_bounded_and_rejected_jobs_release_their_guard() {
    let queue = Queue::default();
    for _ in 0..32 {
        queue.push(Box::new(|| {})).unwrap();
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let guard = Guard(dropped.clone());
    assert!(queue
        .push(Box::new(move || {
            let _guard = guard;
        }))
        .is_err());
    assert!(dropped.load(Ordering::Acquire));
    queue.close();
}
