use fono_stt_protocol::{RequestMeta, WorkerRequest, MAX_REQUEST_FRAME_BYTES, PROTOCOL_VERSION};
use std::collections::BTreeMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{
    mpsc::{self, Receiver},
    Arc, Mutex,
};

pub(crate) struct Job {
    pub request: WorkerRequest,
    pub cancelled: Arc<AtomicBool>,
}
type Cancellations = Arc<Mutex<BTreeMap<String, Arc<AtomicBool>>>>;

pub(crate) fn start(stdout: Arc<Mutex<io::Stdout>>) -> (Receiver<Job>, Cancellations) {
    let (sender, receiver) = mpsc::sync_channel(2);
    let cancellations = Cancellations::default();
    let reader_cancellations = cancellations.clone();
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let mut input = stdin.lock();
        loop {
            let line = match crate::framing::read_limited_line(&mut input, MAX_REQUEST_FRAME_BYTES)
            {
                Ok(Some(line)) => line,
                Ok(None) => break,
                Err(error) => {
                    crate::write_response(
                        &stdout,
                        &crate::runtime::error(
                            RequestMeta::new("unparsed", None),
                            "request_frame",
                            error.to_string(),
                        ),
                    );
                    continue;
                }
            };
            let request: WorkerRequest = match serde_json::from_str(&line) {
                Ok(request) => request,
                Err(error) => {
                    crate::write_response(
                        &stdout,
                        &crate::runtime::error(
                            RequestMeta::new("unparsed", None),
                            "request_json",
                            error.to_string(),
                        ),
                    );
                    continue;
                }
            };
            if request.meta().protocol_version != PROTOCOL_VERSION {
                crate::write_response(
                    &stdout,
                    &crate::runtime::error(
                        request.meta().clone(),
                        "protocol_version",
                        "unsupported protocol version".into(),
                    ),
                );
                continue;
            }
            if let WorkerRequest::CancelRequest {
                target_request_id, ..
            } = request
            {
                if let Some(signal) = reader_cancellations
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get(&target_request_id)
                {
                    signal.store(true, Ordering::Release);
                }
                continue;
            }
            let cancelled = Arc::new(AtomicBool::new(false));
            let meta = request.meta().clone();
            {
                let mut registry = reader_cancellations
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                if registry.contains_key(&meta.request_id) {
                    drop(registry);
                    crate::write_response(
                        &stdout,
                        &crate::runtime::error(meta, "busy", "duplicate request id".into()),
                    );
                    continue;
                }
                registry.insert(meta.request_id.clone(), cancelled.clone());
            }
            if sender.try_send(Job { request, cancelled }).is_err() {
                reader_cancellations
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(&meta.request_id);
                crate::write_response(
                    &stdout,
                    &crate::runtime::error(meta, "busy", "worker queue is full".into()),
                );
            }
        }
        for signal in reader_cancellations
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
        {
            signal.store(true, Ordering::Release);
        }
    });
    (receiver, cancellations)
}
