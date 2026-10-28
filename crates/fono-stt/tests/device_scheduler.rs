use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use fono_core::OperationCancellation;
use fono_stt::error::SttError;
use fono_stt::scheduler::{DeviceId, DeviceScheduler, QueueClass};

const WAIT: Duration = Duration::from_secs(3);

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "scheduler did not reach the expected state"
        );
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn cpu_and_gpu_have_independent_lanes() {
    let scheduler = Arc::new(DeviceScheduler::new(1, [0]).unwrap());
    let held_cpu = scheduler
        .acquire(
            DeviceId::Cpu,
            QueueClass::Batch,
            &OperationCancellation::default(),
        )
        .unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (acquired_tx, acquired_rx) = mpsc::channel();
    let other = Arc::clone(&scheduler);
    let gpu = thread::spawn(move || {
        started_tx.send(()).unwrap();
        let permit = other
            .acquire(
                DeviceId::Cuda(0),
                QueueClass::Batch,
                &OperationCancellation::default(),
            )
            .unwrap();
        acquired_tx.send(()).unwrap();
        drop(permit);
    });
    started_rx.recv_timeout(WAIT).unwrap();
    let acquired_while_cpu_busy = acquired_rx.recv_timeout(WAIT).is_ok();
    drop(held_cpu);
    gpu.join().unwrap();
    assert!(acquired_while_cpu_busy);
}

#[test]
fn cpu_parallelism_is_bounded_by_its_configured_capacity() {
    let scheduler = Arc::new(DeviceScheduler::new(2, [0]).unwrap());
    let first = scheduler
        .acquire(
            DeviceId::Cpu,
            QueueClass::Batch,
            &OperationCancellation::default(),
        )
        .unwrap();
    let second = scheduler
        .acquire(
            DeviceId::Cpu,
            QueueClass::Batch,
            &OperationCancellation::default(),
        )
        .unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (acquired_tx, acquired_rx) = mpsc::channel();
    let waiting_scheduler = Arc::clone(&scheduler);
    let waiter = thread::spawn(move || {
        started_tx.send(()).unwrap();
        let permit = waiting_scheduler
            .acquire(
                DeviceId::Cpu,
                QueueClass::Batch,
                &OperationCancellation::default(),
            )
            .unwrap();
        acquired_tx.send(()).unwrap();
        drop(permit);
    });
    started_rx.recv_timeout(WAIT).unwrap();
    wait_until(|| {
        scheduler
            .status(DeviceId::Cpu)
            .is_some_and(|status| status.queued_batch == 1)
    });
    assert!(matches!(
        acquired_rx.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    drop(first);
    let acquired_after_slot_freed = acquired_rx.recv_timeout(WAIT).is_ok();
    drop(second);
    waiter.join().unwrap();
    assert!(acquired_after_slot_freed);
}

#[test]
fn interactive_uses_a_spare_cpu_slot_without_preempting_batch() {
    let scheduler = Arc::new(DeviceScheduler::new(2, []).unwrap());
    let job = OperationCancellation::default();
    let batch = scheduler
        .acquire(DeviceId::Cpu, QueueClass::Batch, &job)
        .unwrap();
    let interactive = scheduler
        .acquire(
            DeviceId::Cpu,
            QueueClass::Interactive,
            &OperationCancellation::default(),
        )
        .unwrap();
    let status = scheduler.status(DeviceId::Cpu).unwrap();
    assert_eq!(status.active_batch, 1);
    assert_eq!(status.active_interactive, 1);
    assert!(!batch.cancellation.is_cancelled());
    assert!(!job.is_cancelled());
    drop(interactive);
    drop(batch);
}

#[test]
fn one_interactive_waiter_preempts_only_one_of_two_active_cpu_batches() {
    let scheduler = Arc::new(DeviceScheduler::new(2, []).unwrap());
    let first_job = OperationCancellation::default();
    let second_job = OperationCancellation::default();
    let first = scheduler
        .acquire(DeviceId::Cpu, QueueClass::Batch, &first_job)
        .unwrap();
    let second = scheduler
        .acquire(DeviceId::Cpu, QueueClass::Batch, &second_job)
        .unwrap();
    let (acquired_tx, acquired_rx) = mpsc::channel();
    let waiting_scheduler = Arc::clone(&scheduler);
    let waiter = thread::spawn(move || {
        let _permit = waiting_scheduler
            .acquire(
                DeviceId::Cpu,
                QueueClass::Interactive,
                &OperationCancellation::default(),
            )
            .unwrap();
        acquired_tx.send(()).unwrap();
    });
    wait_until(|| {
        scheduler
            .status(DeviceId::Cpu)
            .is_some_and(|status| status.queued_interactive == 1)
    });
    wait_until(|| first.cancellation.is_cancelled() || second.cancellation.is_cancelled());
    let cancelled_count = usize::from(first.cancellation.is_cancelled())
        + usize::from(second.cancellation.is_cancelled());
    assert_eq!(
        cancelled_count, 1,
        "one free slot is enough for this waiter"
    );
    assert!(!first_job.is_cancelled());
    assert!(!second_job.is_cancelled());
    let (cancelled, retained) = if first.cancellation.is_cancelled() {
        (first, second)
    } else {
        (second, first)
    };
    drop(cancelled);
    let entered_with_one_batch_still_active = acquired_rx.recv_timeout(WAIT).is_ok();
    assert!(!retained.cancellation.is_cancelled());
    drop(retained);
    waiter.join().unwrap();
    assert!(entered_with_one_batch_still_active);
}

#[test]
fn interactive_overtakes_queued_batch_on_the_same_device() {
    let scheduler = Arc::new(DeviceScheduler::new(1, [0]).unwrap());
    let active = scheduler
        .acquire(
            DeviceId::Cuda(0),
            QueueClass::Batch,
            &OperationCancellation::default(),
        )
        .unwrap();
    let (order_tx, order_rx) = mpsc::channel();
    let (batch_started_tx, batch_started_rx) = mpsc::channel();
    let batch_scheduler = Arc::clone(&scheduler);
    let batch_order = order_tx.clone();
    let batch = thread::spawn(move || {
        batch_started_tx.send(()).unwrap();
        let _permit = batch_scheduler
            .acquire(
                DeviceId::Cuda(0),
                QueueClass::Batch,
                &OperationCancellation::default(),
            )
            .unwrap();
        batch_order.send(QueueClass::Batch).unwrap();
    });
    batch_started_rx.recv_timeout(WAIT).unwrap();
    wait_until(|| {
        scheduler
            .status(DeviceId::Cuda(0))
            .is_some_and(|status| status.queued_batch == 1)
    });

    let (interactive_started_tx, interactive_started_rx) = mpsc::channel();
    let interactive_scheduler = Arc::clone(&scheduler);
    let interactive = thread::spawn(move || {
        interactive_started_tx.send(()).unwrap();
        let _permit = interactive_scheduler
            .acquire(
                DeviceId::Cuda(0),
                QueueClass::Interactive,
                &OperationCancellation::default(),
            )
            .unwrap();
        order_tx.send(QueueClass::Interactive).unwrap();
    });
    interactive_started_rx.recv_timeout(WAIT).unwrap();
    wait_until(|| {
        scheduler
            .status(DeviceId::Cuda(0))
            .is_some_and(|status| status.queued_interactive == 1)
    });
    wait_until(|| active.cancellation.is_cancelled());
    drop(active);
    let first = order_rx.recv_timeout(WAIT).unwrap();
    let second = order_rx.recv_timeout(WAIT).unwrap();
    interactive.join().unwrap();
    batch.join().unwrap();
    assert_eq!(first, QueueClass::Interactive);
    assert_eq!(second, QueueClass::Batch);
}

#[test]
fn batch_requests_enter_in_fifo_order_on_one_device() {
    let scheduler = Arc::new(DeviceScheduler::new(1, [0]).unwrap());
    let active = scheduler
        .acquire(
            DeviceId::Cuda(0),
            QueueClass::Batch,
            &OperationCancellation::default(),
        )
        .unwrap();
    let (order_tx, order_rx) = mpsc::channel();
    let first_scheduler = Arc::clone(&scheduler);
    let first_order = order_tx.clone();
    let first = thread::spawn(move || {
        let _permit = first_scheduler
            .acquire(
                DeviceId::Cuda(0),
                QueueClass::Batch,
                &OperationCancellation::default(),
            )
            .unwrap();
        first_order.send(1).unwrap();
    });
    wait_until(|| {
        scheduler
            .status(DeviceId::Cuda(0))
            .is_some_and(|status| status.queued_batch == 1)
    });
    let second_scheduler = Arc::clone(&scheduler);
    let second = thread::spawn(move || {
        let _permit = second_scheduler
            .acquire(
                DeviceId::Cuda(0),
                QueueClass::Batch,
                &OperationCancellation::default(),
            )
            .unwrap();
        order_tx.send(2).unwrap();
    });
    wait_until(|| {
        scheduler
            .status(DeviceId::Cuda(0))
            .is_some_and(|status| status.queued_batch == 2)
    });
    drop(active);
    let first_admitted = order_rx.recv_timeout(WAIT).unwrap();
    let second_admitted = order_rx.recv_timeout(WAIT).unwrap();
    first.join().unwrap();
    second.join().unwrap();
    assert_eq!((first_admitted, second_admitted), (1, 2));
}

#[test]
fn interactive_preempts_only_the_active_batch_window() {
    let scheduler = Arc::new(DeviceScheduler::new(1, [0]).unwrap());
    let job = OperationCancellation::default();
    let batch = scheduler
        .acquire(DeviceId::Cuda(0), QueueClass::Batch, &job)
        .unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (acquired_tx, acquired_rx) = mpsc::channel();
    let waiting_scheduler = Arc::clone(&scheduler);
    let interactive = thread::spawn(move || {
        started_tx.send(()).unwrap();
        let _permit = waiting_scheduler
            .acquire(
                DeviceId::Cuda(0),
                QueueClass::Interactive,
                &OperationCancellation::default(),
            )
            .unwrap();
        acquired_tx.send(()).unwrap();
    });
    started_rx.recv_timeout(WAIT).unwrap();
    wait_until(|| {
        scheduler
            .status(DeviceId::Cuda(0))
            .is_some_and(|status| status.queued_interactive == 1)
    });
    wait_until(|| batch.cancellation.is_cancelled());
    assert!(!job.is_cancelled(), "preemption must not cancel the job");
    drop(batch);
    let acquired_after_window = acquired_rx.recv_timeout(WAIT).is_ok();
    interactive.join().unwrap();
    assert!(acquired_after_window);
}

#[test]
fn cancelling_a_queued_request_clears_its_place() {
    let scheduler = Arc::new(DeviceScheduler::new(1, [0]).unwrap());
    let active = scheduler
        .acquire(
            DeviceId::Cuda(0),
            QueueClass::Batch,
            &OperationCancellation::default(),
        )
        .unwrap();
    let queued_cancel = OperationCancellation::default();
    let waiter_cancel = queued_cancel.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let (cancelled_tx, cancelled_rx) = mpsc::channel();
    let waiting_scheduler = Arc::clone(&scheduler);
    let waiter = thread::spawn(move || {
        started_tx.send(()).unwrap();
        let result =
            waiting_scheduler.acquire(DeviceId::Cuda(0), QueueClass::Batch, &waiter_cancel);
        cancelled_tx
            .send(matches!(result, Err(SttError::Cancelled(_))))
            .unwrap();
    });
    started_rx.recv_timeout(WAIT).unwrap();
    wait_until(|| {
        scheduler
            .status(DeviceId::Cuda(0))
            .is_some_and(|status| status.queued_batch == 1)
    });
    queued_cancel.cancel();
    let cancelled_before_slot_freed = cancelled_rx.recv_timeout(WAIT).unwrap_or(false);
    let queue_cleared = scheduler
        .status(DeviceId::Cuda(0))
        .is_some_and(|status| status.queued_batch == 0);
    drop(active);
    waiter.join().unwrap();
    assert!(cancelled_before_slot_freed);
    assert!(queue_cleared);
    let _next = scheduler
        .acquire(
            DeviceId::Cuda(0),
            QueueClass::Batch,
            &OperationCancellation::default(),
        )
        .unwrap();
}

#[test]
fn cuda_devices_have_independent_single_slot_lanes() {
    let scheduler = Arc::new(DeviceScheduler::new(1, [0, 1]).unwrap());
    let first_gpu = scheduler
        .acquire(
            DeviceId::Cuda(0),
            QueueClass::Batch,
            &OperationCancellation::default(),
        )
        .unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (acquired_tx, acquired_rx) = mpsc::channel();
    let other = Arc::clone(&scheduler);
    let second_gpu = thread::spawn(move || {
        started_tx.send(()).unwrap();
        let permit = other
            .acquire(
                DeviceId::Cuda(1),
                QueueClass::Batch,
                &OperationCancellation::default(),
            )
            .unwrap();
        acquired_tx.send(()).unwrap();
        drop(permit);
    });
    started_rx.recv_timeout(WAIT).unwrap();
    let acquired_while_first_gpu_busy = acquired_rx.recv_timeout(WAIT).is_ok();
    drop(first_gpu);
    second_gpu.join().unwrap();
    assert!(acquired_while_first_gpu_busy);
}
