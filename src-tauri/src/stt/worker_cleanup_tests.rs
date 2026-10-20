#![cfg(windows)]

use super::*;

// Covers the complete cleanup path, with headroom for parallel Windows fixtures.
const CLEANUP_TEST_BOUND: Duration = Duration::from_secs(5);

const SHUTDOWN_ACK: &str =
    "echo {\"type\":\"shutting_down\",\"protocol_version\":4,\"request_id\":\"shutdown-3\"}";

struct ResetForcedKillFailure(Arc<WorkerChild>);

impl Drop for ResetForcedKillFailure {
    fn drop(&mut self) {
        self.0.force_kill_fail.store(false, Ordering::Release);
    }
}

fn start_with_supervisor(
    fixture: &WorkerFixture,
    supervisor: &Arc<WorkerCleanupSupervisor>,
) -> WorkerSession {
    WorkerSession::start_with_timeouts_and_supervisor(
        fixture.script.clone(),
        BackendKind::Cuda,
        Path::new("fixture-model.bin"),
        TEST_TIMEOUTS,
        Arc::clone(supervisor),
    )
    .expect("fixture worker completes handshake under its cleanup supervisor")
}

fn wait_for_marker(marker: &Path) {
    let deadline = Instant::now() + EOF_FIXTURE_TIMEOUT;
    while !marker.exists() {
        assert!(
            Instant::now() < deadline,
            "fixture did not reach its protocol barrier"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

fn wait_for_confirmed_cleanup(child: &Arc<WorkerChild>, supervisor: &WorkerCleanupSupervisor) {
    let deadline = Instant::now() + CLEANUP_TEST_BOUND;
    while supervisor.is_pending() || !matches!(child.try_wait(), Ok(Some(_))) {
        assert!(
            Instant::now() < deadline,
            "worker cleanup was not confirmed before the deadline"
        );
        thread::sleep(CANCELLATION_POLL_INTERVAL);
    }
}

#[test]
fn acknowledged_shutdown_without_exit_is_force_killed_within_the_cleanup_bound() {
    let fixture = WorkerFixture::create(&format!("{SHUTDOWN_ACK}\r\n:hang\r\ngoto hang"));
    let supervisor = Arc::new(WorkerCleanupSupervisor::default());
    let mut session = start_with_supervisor(&fixture, &supervisor);
    let child = Arc::clone(&session.child);
    let started = Instant::now();

    assert!(matches!(session.shutdown(), WorkerCleanupStatus::Exited));

    assert!(started.elapsed() < CLEANUP_TEST_BOUND);
    assert!(child.kill_count.load(Ordering::Acquire) > 0);
    assert!(matches!(child.try_wait(), Ok(Some(_))));
    assert!(!supervisor.is_pending());
    assert_session_is_terminated(&mut session);
}

#[test]
fn graceful_shutdown_reaps_a_successful_exit_without_force_killing() {
    let fixture = WorkerFixture::create(&format!(
        "{SHUTDOWN_ACK}\r\n>\"%~dp0shutdown-acknowledged\" echo ready\r\n:await_release\r\nif not exist \"%~dp0shutdown-release\" goto await_release\r\nexit /b 0"
    ));
    let supervisor = Arc::new(WorkerCleanupSupervisor::default());
    let mut session = start_with_supervisor(&fixture, &supervisor);
    let child = Arc::clone(&session.child);
    let writer = session
        .stdin_tx
        .as_ref()
        .expect("open fixture stdin")
        .clone();
    let acknowledged = fixture.directory.join("shutdown-acknowledged");
    let release = fixture.directory.join("shutdown-release");
    // cmd can consume the JSON before the writer's newline syscall. Keep the
    // fixture alive until a FIFO barrier proves the entire shutdown frame was sent.
    let barrier = thread::spawn(move || {
        wait_for_marker(&acknowledged);
        let (result_tx, result_rx) = bounded(1);
        writer
            .send_timeout(
                WorkerWrite {
                    line: String::new(),
                    result_tx,
                },
                EOF_FIXTURE_TIMEOUT,
            )
            .expect("enqueue barrier after the shutdown frame");
        result_rx
            .recv_timeout(EOF_FIXTURE_TIMEOUT)
            .expect("writer acknowledges the shutdown FIFO barrier")
            .expect("shutdown frame reached open fixture stdin completely");
        fs::write(release, []).expect("release graceful fixture after the complete FIFO write");
    });
    let started = Instant::now();

    assert!(matches!(session.shutdown(), WorkerCleanupStatus::Exited));

    barrier
        .join()
        .expect("shutdown barrier controller completes");
    assert!(started.elapsed() < CLEANUP_TEST_BOUND);
    assert_eq!(child.kill_count.load(Ordering::Acquire), 0);
    assert!(child
        .try_wait()
        .expect("read graceful worker exit")
        .expect("worker exited")
        .success());
    assert!(!supervisor.is_pending());
    assert_session_is_terminated(&mut session);
}

#[test]
fn cancelled_request_cleans_up_before_a_fresh_session_uses_the_same_supervisor() {
    let hung =
        WorkerFixture::create(">\"%~dp0transcribe-started\" echo ready\r\n:hang\r\ngoto hang");
    let supervisor = Arc::new(WorkerCleanupSupervisor::default());
    let mut session = start_with_supervisor(&hung, &supervisor);
    let previous_child = Arc::clone(&session.child);
    let coordinator = Arc::new(OperationCoordinator::new());
    let operation = coordinator
        .start(OperationSource::Ui)
        .expect("start cancellable fixture request");
    let cancellation = coordinator
        .cancellation(operation.id)
        .expect("request cancellation signal");
    let coordinator_for_cancel = Arc::clone(&coordinator);
    let observed = hung.directory.join("transcribe-started");
    let cancel = thread::spawn(move || {
        wait_for_marker(&observed);
        coordinator_for_cancel.cancel(operation.id);
    });
    let started = Instant::now();

    let error = session
        .transcribe_cancellable(&[0; 160], "auto", &cancellation)
        .expect_err("cancelled request must stop waiting for worker inference");

    cancel.join().expect("cancellation controller completes");
    assert!(started.elapsed() < CLEANUP_TEST_BOUND);
    assert!(matches!(error, AppError::Cancelled(_)));
    wait_for_confirmed_cleanup(&previous_child, &supervisor);
    assert_session_is_terminated(&mut session);

    let fresh = WorkerFixture::create(
        "echo {\"type\":\"pong\",\"protocol_version\":4,\"request_id\":\"ping-3\",\"backend\":\"cuda\"}\r\nset /p request=\r\necho {\"type\":\"shutting_down\",\"protocol_version\":4,\"request_id\":\"shutdown-4\"}\r\n:hang\r\ngoto hang",
    );
    let mut replacement = start_with_supervisor(&fresh, &supervisor);
    replacement
        .ping()
        .expect("the replacement owns a fresh request sequence");
    assert!(matches!(previous_child.try_wait(), Ok(Some(_))));
    assert!(matches!(
        replacement.shutdown(),
        WorkerCleanupStatus::Exited
    ));
    assert!(!supervisor.is_pending());
    assert_session_is_terminated(&mut replacement);
}

#[test]
fn mailbox_shutdown_reaps_an_owner_whose_worker_acknowledges_but_never_exits() {
    let fixture = WorkerFixture::create(&format!("{SHUTDOWN_ACK}\r\n:hang\r\ngoto hang"));
    let supervisor = Arc::new(WorkerCleanupSupervisor::default());
    let session = start_with_supervisor(&fixture, &supervisor);
    let child = Arc::clone(&session.child);
    let mailbox = WorkerMailbox::start(session);
    let started = Instant::now();

    assert!(matches!(mailbox.shutdown(), WorkerCleanupStatus::Exited));

    assert!(started.elapsed() < CLEANUP_TEST_BOUND);
    assert!(child.kill_count.load(Ordering::Acquire) > 0);
    assert!(matches!(child.try_wait(), Ok(Some(_))));
    assert!(!mailbox.alive.load(Ordering::Acquire));
    assert!(!supervisor.is_pending());
    let dropping = Instant::now();
    drop(mailbox);
    assert!(dropping.elapsed() < CLEANUP_TEST_BOUND);
}

#[test]
fn failed_force_kill_keeps_cleanup_pending_and_blocks_replacement_until_exit_is_confirmed() {
    let fixture = WorkerFixture::create(&format!("{SHUTDOWN_ACK}\r\n:hang\r\ngoto hang"));
    let supervisor = Arc::new(WorkerCleanupSupervisor::default());
    let mut session = start_with_supervisor(&fixture, &supervisor);
    let previous_child = Arc::clone(&session.child);
    previous_child
        .force_kill_fail
        .store(true, Ordering::Release);
    let failure = ResetForcedKillFailure(Arc::clone(&previous_child));
    let started = Instant::now();

    assert!(matches!(session.shutdown(), WorkerCleanupStatus::Pending));

    assert!(started.elapsed() < CLEANUP_TEST_BOUND);
    assert!(supervisor.is_pending());
    assert!(matches!(previous_child.try_wait(), Ok(None)));

    let fresh = WorkerFixture::create(&format!("{SHUTDOWN_ACK}\r\n:hang\r\ngoto hang"));
    let script = fs::read_to_string(&fresh.script).expect("read replacement fixture");
    fs::write(
        &fresh.script,
        format!("@echo off\r\n>\"%~dp0spawn-observed\" echo started\r\n{script}"),
    )
    .expect("record whether replacement is spawned");
    let rejected = WorkerSession::start_with_timeouts_and_supervisor(
        fresh.script.clone(),
        BackendKind::Cuda,
        Path::new("fixture-model.bin"),
        TEST_TIMEOUTS,
        Arc::clone(&supervisor),
    );
    assert!(matches!(rejected, Err(AppError::Busy(_))));
    assert!(!fresh.directory.join("spawn-observed").exists());

    drop(failure);
    wait_for_confirmed_cleanup(&previous_child, &supervisor);
    assert!(matches!(previous_child.try_wait(), Ok(Some(_))));
    let mut replacement = start_with_supervisor(&fresh, &supervisor);
    assert!(fresh.directory.join("spawn-observed").exists());
    let replacement_child = Arc::clone(&replacement.child);
    replacement.shutdown();
    // A loaded Windows runner may return the safe Pending result while its
    // native I/O threads finish; replacement remains fenced until confirmed.
    wait_for_confirmed_cleanup(&replacement_child, &supervisor);
    assert!(!supervisor.is_pending());
    assert_session_is_terminated(&mut replacement);
}

#[test]
fn adaptive_release_keeps_pending_worker_residency_and_blocks_auto_loading_until_cleanup() {
    let fixture = WorkerFixture::create(&format!("{SHUTDOWN_ACK}\r\n:hang\r\ngoto hang"));
    let supervisor = Arc::new(WorkerCleanupSupervisor::default());
    let session = start_with_supervisor(&fixture, &supervisor);
    let child = Arc::clone(&session.child);
    child.force_kill_fail.store(true, Ordering::Release);
    let failure = ResetForcedKillFailure(Arc::clone(&child));
    let mut engine = crate::stt::SttEngine::new();
    engine.cleanup = Arc::clone(&supervisor);
    *engine.state.lock() = crate::stt::routing::EngineState::Worker {
        session: Arc::new(WorkerMailbox::start(session)),
        device: "CUDA",
    };
    let ready = crate::stt::SttReadiness::Ready {
        device: "CUDA".into(),
    };
    engine.set_readiness(ready.clone());
    let initial = engine.gpu_residency();
    assert!(initial.loaded && initial.resident && initial.exclusive);
    assert!(!initial.cleanup_pending);
    assert_eq!(initial.generation, 0);
    let started = Instant::now();

    assert!(!engine.release_idle_gpu(0, |detach| detach()));

    assert!(started.elapsed() < CLEANUP_TEST_BOUND);
    assert!(supervisor.is_pending());
    let pending = engine.gpu_residency();
    assert!(pending.cleanup_pending && pending.loaded && pending.resident);
    assert!(!pending.exclusive);
    assert!(engine.is_loaded());
    let retry = engine.ensure_loaded(
        Path::new("unused-cleanup-fixture.bin"),
        crate::types::AccelerationMode::Auto,
        &crate::stt::WorkerPaths::default(),
    );
    assert!(matches!(retry, Err(AppError::Busy(_))));
    assert_eq!(engine.readiness(), ready);

    drop(failure);
    wait_for_confirmed_cleanup(&child, &supervisor);
    let cleaned = engine.gpu_residency();
    assert!(!cleaned.cleanup_pending);
    assert!(!cleaned.loaded && !cleaned.resident && !cleaned.exclusive);
    assert!(!engine.is_loaded());
    assert!(matches!(
        *engine.state.lock(),
        crate::stt::routing::EngineState::Empty
    ));
    assert_eq!(engine.readiness(), ready);
}
