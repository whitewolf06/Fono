#![cfg_attr(windows, windows_subsystem = "windows")]
mod control;
mod framing;
mod runtime;
use fono_stt_protocol::WorkerResponse;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
fn write_response(stdout: &Arc<Mutex<io::Stdout>>, response: &WorkerResponse) {
    if let Ok(json) = serde_json::to_string(response) {
        let mut output = stdout
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = writeln!(output, "{json}");
        let _ = output.flush();
    }
}
fn main() {
    let stdout = Arc::new(Mutex::new(io::stdout()));
    let (queue, cancellations) = control::start(stdout.clone());
    let mut loaded = None;
    while let Ok(job) = queue.recv() {
        let request_id = job.request.meta().request_id.clone();
        let (response, shutdown) = runtime::handle(job.request, &mut loaded, &job.cancelled);
        write_response(&stdout, &response);
        cancellations
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&request_id);
        if shutdown {
            break;
        }
    }
}
