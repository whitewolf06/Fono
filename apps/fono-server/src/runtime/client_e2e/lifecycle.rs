//! Disconnects close real client/server sockets. Inference deliberately keeps
//! running after observing cancellation, until a separate release is granted.

use super::{model, options, Host, CLIENT_KEY, MODEL, TOKEN};
use crate::{
    recognition_options::RecognitionOutput,
    recognize::{AdmittedRecognition, InteractiveRecognizer},
};
use fono_core::OperationCancellation;
use fono_service::transcription_jobs::ClientInflightLimit;
use fono_stt::{
    error::{SttError, SttResult},
    remote::{RemoteEndpoint, RemoteErrorKind, RemoteRecognizer, RemoteTimeouts},
    ActivityGuard, ActivityLease,
};
use fono_types::{Transcript, TranscriptionResult, TranscriptionServiceError};
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{oneshot, Notify},
    task::{JoinHandle, JoinSet},
};

const FRESH_RESULT: &str = "fresh lifecycle result";
const LATE_RESULT: &str = "late cancelled lifecycle result";

#[derive(Default)]
struct CountingActivity(Arc<AtomicUsize>);

struct CountedLease(Arc<AtomicUsize>);

impl Drop for CountedLease {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl CountingActivity {
    fn active(&self) -> usize {
        self.0.load(Ordering::Acquire)
    }
}

impl ActivityGuard for CountingActivity {
    fn lease(&self) -> SttResult<ActivityLease> {
        Ok(self.cleanup_lease())
    }

    fn cleanup_lease(&self) -> ActivityLease {
        self.0.fetch_add(1, Ordering::AcqRel);
        ActivityLease::new(CountedLease(self.0.clone()))
    }
}

struct HeldRecognizer {
    limit: ClientInflightLimit,
    calls: AtomicUsize,
    started: AtomicBool,
    cancellation_observed: AtomicBool,
    completed: AtomicUsize,
    released: Mutex<bool>,
    release_changed: Condvar,
}

impl HeldRecognizer {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            limit: ClientInflightLimit::new(1),
            calls: AtomicUsize::new(0),
            started: AtomicBool::new(false),
            cancellation_observed: AtomicBool::new(false),
            completed: AtomicUsize::new(0),
            released: Mutex::new(false),
            release_changed: Condvar::new(),
        })
    }

    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.release_changed.notify_all();
    }

    fn wait_for_release(
        &self,
        cancellation: &OperationCancellation,
    ) -> Result<(), TranscriptionServiceError> {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut released = self.released.lock().unwrap();
        while !*released {
            if cancellation.is_cancelled() {
                self.cancellation_observed.store(true, Ordering::Release);
            }
            if Instant::now() >= deadline {
                return Err(TranscriptionServiceError::Failed(
                    "test inference was not released".into(),
                ));
            }
            released = self
                .release_changed
                .wait_timeout(released, Duration::from_millis(5))
                .unwrap()
                .0;
        }
        Ok(())
    }
}

impl InteractiveRecognizer for HeldRecognizer {
    fn admit(
        self: Arc<Self>,
        client_key: &str,
    ) -> Result<AdmittedRecognition, TranscriptionServiceError> {
        // The same production admission primitive is captured by actual work,
        // rather than by the HTTP waiter or an independent test flag.
        let lease = self.limit.try_acquire(client_key)?;
        Ok(Box::new(move |request, _class, _options, cancellation| {
            let call = self.calls.fetch_add(1, Ordering::AcqRel);
            let outcome = if call == 0 {
                self.started.store(true, Ordering::Release);
                self.wait_for_release(&cancellation).map(|()| LATE_RESULT)
            } else {
                Ok(FRESH_RESULT)
            };
            // Success after cancellation models native work which cannot stop
            // immediately. Its late result must never reach a later request.
            let outcome = outcome.map(|text| RecognitionOutput {
                result: TranscriptionResult::from_transcript(
                    Transcript {
                        text: text.into(),
                        detected_language: Some(request.language),
                        transcribe_secs: None,
                        audio_secs: Some(request.pcm_samples.len() as f32 / 16_000.0),
                        device: Some("CPU".into()),
                        remote: None,
                    },
                    MODEL.into(),
                ),
                words: Vec::new(),
                device: Some("cpu".into()),
            });
            drop(lease);
            self.completed.fetch_add(1, Ordering::AcqRel);
            outcome
        }))
    }
}

struct ReleaseOnDrop(Arc<HeldRecognizer>);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        // Panic cleanup also releases a worker which has not observed cancel.
        self.0.release();
    }
}

async fn until(mut predicate: impl FnMut() -> bool, message: &str) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect(message);
}

async fn connect(
    endpoint: RemoteEndpoint,
    activity: Arc<CountingActivity>,
) -> Arc<RemoteRecognizer> {
    Arc::new(
        tokio::task::spawn_blocking(move || {
            RemoteRecognizer::connect(
                endpoint,
                model(),
                activity,
                &OperationCancellation::default(),
            )
        })
        .await
        .unwrap()
        .unwrap(),
    )
}

fn transcribe(
    client: Arc<RemoteRecognizer>,
    cancellation: OperationCancellation,
) -> JoinHandle<SttResult<Transcript>> {
    tokio::task::spawn_blocking(move || client.transcribe(&[1024; 1600], options(), &cancellation))
}

async fn request_result(task: JoinHandle<SttResult<Transcript>>) -> SttResult<Transcript> {
    tokio::time::timeout(Duration::from_secs(8), task)
        .await
        .expect("desktop HTTP request did not finish")
        .unwrap()
}

#[derive(Clone, Copy)]
enum Interrupt {
    Cancel,
    Deadline,
    TransportBreak,
}

async fn interrupted_work_retains_admission(interrupt: Interrupt) {
    let recognizer = HeldRecognizer::new();
    let _release_on_drop = ReleaseOnDrop(recognizer.clone());
    let host = Host::start(recognizer.clone()).await;
    let forwarder = if matches!(interrupt, Interrupt::TransportBreak) {
        Some(Forwarder::start(&host.url).await)
    } else {
        None
    };
    let mut endpoint = match &forwarder {
        Some(proxy) => RemoteEndpoint::new(&proxy.url, TOKEN).unwrap(),
        None => host.endpoint(TOKEN),
    };
    if matches!(interrupt, Interrupt::Deadline) {
        endpoint = endpoint
            .with_timeouts(RemoteTimeouts {
                connect: Duration::from_secs(1),
                response_base: Duration::from_secs(3),
                audio_multiplier: 0.0,
            })
            .unwrap();
    }
    let activity = Arc::new(CountingActivity::default());
    let client = connect(endpoint, activity.clone()).await;
    assert_eq!(activity.active(), 0, "discovery HTTP work must be complete");
    let cancellation = OperationCancellation::default();
    let pending = transcribe(client.clone(), cancellation.clone());
    until(
        || recognizer.started.load(Ordering::Acquire),
        "recognition never reached the server worker",
    )
    .await;
    assert_eq!(activity.active(), 1);
    assert!(host.activity.active() >= 1);

    match interrupt {
        Interrupt::Cancel => cancellation.cancel(),
        Interrupt::Deadline => {}
        Interrupt::TransportBreak => forwarder.as_ref().unwrap().cut(),
    }
    let error = request_result(pending).await.unwrap_err();
    match interrupt {
        Interrupt::Cancel => assert!(matches!(error, SttError::Cancelled(_))),
        Interrupt::Deadline => assert!(matches!(
            error,
            SttError::Remote(error) if error.kind == RemoteErrorKind::Timeout
        )),
        Interrupt::TransportBreak => assert!(matches!(
            error,
            SttError::Remote(error) if error.kind == RemoteErrorKind::Network
        )),
    }
    // The remote client releases its lease only after its HTTP future is gone.
    // The server still owns native work and cannot advertise an empty fence.
    assert_eq!(activity.active(), 0);
    until(
        || recognizer.cancellation_observed.load(Ordering::Acquire),
        "closed HTTP transport did not cancel server recognition",
    )
    .await;
    assert!(host.activity.active() >= 1);
    assert_eq!(recognizer.completed.load(Ordering::Acquire), 0);
    assert!(matches!(
        recognizer.limit.try_acquire(CLIENT_KEY),
        Err(TranscriptionServiceError::Busy(_))
    ));

    // An explicit retry cannot displace still-running cancelled native work.
    let busy = request_result(transcribe(client.clone(), OperationCancellation::default()))
        .await
        .unwrap_err();
    assert!(matches!(
        busy,
        SttError::Remote(error)
            if error.kind == RemoteErrorKind::Busy
                && error.status == Some(429)
                && error.retry_after_seconds == Some(1)
    ));
    assert_eq!(activity.active(), 0);
    assert_eq!(recognizer.calls.load(Ordering::Acquire), 1);
    assert!(host.activity.active() >= 1);

    recognizer.release();
    until(
        || recognizer.completed.load(Ordering::Acquire) == 1 && host.activity.active() == 0,
        "actual worker did not release the server admission fence",
    )
    .await;
    let fresh = request_result(transcribe(client.clone(), OperationCancellation::default()))
        .await
        .unwrap();
    assert_eq!(fresh.text, FRESH_RESULT);
    assert_ne!(fresh.text, LATE_RESULT);
    assert_eq!(recognizer.calls.load(Ordering::Acquire), 2);
    assert_eq!(recognizer.completed.load(Ordering::Acquire), 2);
    assert_eq!(activity.active(), 0);
    until(
        || host.activity.active() == 0,
        "retry did not release server activity",
    )
    .await;
    drop(client);
    if let Some(proxy) = forwarder {
        proxy.stop().await;
    }
    host.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn desktop_cancel_retains_actual_server_work_until_it_finishes() {
    interrupted_work_retains_admission(Interrupt::Cancel).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn desktop_deadline_retains_actual_server_work_until_it_finishes() {
    interrupted_work_retains_admission(Interrupt::Deadline).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn broken_tcp_transport_retains_server_work_and_allows_explicit_retry() {
    interrupted_work_retains_admission(Interrupt::TransportBreak).await;
}

/// Owns both sides of every forwarded connection. Cutting them tests a real
/// TCP disconnect while leaving both the production listener and proxy alive.
struct Forwarder {
    url: String,
    cut: Arc<Notify>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl Forwarder {
    async fn start(upstream_url: &str) -> Self {
        let upstream: SocketAddr = upstream_url
            .strip_prefix("http://")
            .unwrap()
            .trim_end_matches('/')
            .parse()
            .unwrap();
        assert!(upstream.ip().is_loopback());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let cut = Arc::new(Notify::new());
        let connection_cut = cut.clone();
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    _ = &mut stopped => break,
                    accepted = listener.accept() => {
                        let (mut incoming, _) = accepted.unwrap();
                        let cut = connection_cut.clone();
                        connections.spawn(async move {
                            let mut outgoing = TcpStream::connect(upstream).await.unwrap();
                            tokio::select! {
                                _ = cut.notified() => {},
                                _ = tokio::io::copy_bidirectional(&mut incoming, &mut outgoing) => {},
                            }
                            // Drop both owned sockets here. This is the fault;
                            // no server task or HTTP handler is aborted.
                        });
                    }
                    Some(result) = connections.join_next() => result.unwrap(),
                }
            }
            connection_cut.notify_waiters();
            while let Some(result) = connections.join_next().await {
                result.unwrap();
            }
        });
        Self {
            url,
            cut,
            stop: Some(stop),
            task: Some(task),
        }
    }

    fn cut(&self) {
        // Called only after the server has entered inference through this
        // connection, so its forwarding select is already subscribed.
        self.cut.notify_waiters();
    }

    async fn stop(mut self) {
        let _ = self.stop.take().unwrap().send(());
        tokio::time::timeout(Duration::from_secs(5), self.task.take().unwrap())
            .await
            .expect("TCP forwarder did not drain")
            .unwrap();
    }
}

impl Drop for Forwarder {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            // Panic cleanup only; never used to create a tested disconnect.
            task.abort();
        }
    }
}
