//! One warm, supervised process. The audio callback never accesses its pipes.
use super::Control;
use crate::error::{AppError, AppResult};
use crossbeam_channel::{bounded, Receiver, Sender};
use serde::Deserialize;
use std::{
    io::{BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::atomic::Ordering,
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub(super) const FRAME: usize = 1280;
const CNN: &str = "a9c4e2595669605325b97b7aeec62781c58073e9966a3850d360328bddaa541c";
const VERIFIER: &str = "d3ea4079b329755bcdd42618cec915aa4952554479eb4b0999689558305ee47e";
// Cold Python/SciPy imports can exceed one minute after a long idle. This is
// Loading only: no audio is subscribed until the complete warmup handshake.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(120);

fn failed() -> AppError {
    failure("initialization")
}
fn failure(reason: &'static str) -> AppError {
    tracing::warn!(
        event = "lira_worker_failed",
        reason,
        "live wake worker failed"
    );
    AppError::Internal(
        "Детектор не ответил или остановился. Выключите пробуждение и включите снова.".into(),
    )
}
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Job(windows::Win32::Foundation::HANDLE);
impl Job {
    fn attach(child: &Child) -> AppResult<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows::{
            core::PCWSTR,
            Win32::{Foundation::HANDLE, System::JobObjects::*},
        };
        // Kill-on-close also covers an unexpected desktop process exit.
        unsafe {
            let job = Self(CreateJobObjectW(None, PCWSTR::null()).map_err(|_| failed())?);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of_val(&info) as u32,
            )
            .map_err(|_| failed())?;
            AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle())).map_err(|_| failed())?;
            Ok(job)
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

pub(super) struct WarmWorker {
    child: OwnedChild,
    job: Option<Job>,
    requests: Option<Sender<Vec<u8>>>,
    responses: Receiver<Result<serde_json::Value, ()>>,
    io: Option<JoinHandle<()>>,
    processed: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    protocol: u32,
    cnn: String,
    verifier: String,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum LoadingStage {
    Runtime,
    Models,
    Warmup,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Loading {
    loading: LoadingStage,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    processed: u64,
    detected: bool,
}

fn read_reply(source: &mut impl Read) -> Result<serde_json::Value, ()> {
    let mut bytes = Vec::new();
    for _ in 0..4096 {
        let mut byte = [0];
        source.read_exact(&mut byte).map_err(|_| ())?;
        if byte[0] == b'\n' {
            return serde_json::from_slice(&bytes).map_err(|_| ());
        }
        bytes.push(byte[0]);
    }
    Err(())
}

fn wait_response(
    responses: &Receiver<Result<serde_json::Value, ()>>,
    control: &Control,
    deadline: Instant,
    timeout_reason: &'static str,
) -> AppResult<serde_json::Value> {
    loop {
        if control.cancelled.load(Ordering::Acquire) {
            return Err(AppError::Cancelled("Пробуждение выключено.".into()));
        }
        // A scheduling pause may outlive the deadline even though the worker
        // already replied. Drain a queued response before declaring failure;
        // the service still rejects stale audio and obsolete operation IDs.
        let wait = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(25));
        match responses.recv_timeout(wait) {
            Ok(value) => {
                if let Some(late) = Instant::now().checked_duration_since(deadline) {
                    tracing::warn!(
                        event = "lira_reply_after_scheduling_gap",
                        late_ms = late.as_millis() as u64,
                        "queued worker reply retained after a scheduling gap"
                    );
                }
                return value.map_err(|_| failure("pipe_read_or_write"));
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return Err(failure("pipe_closed"));
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if Instant::now() >= deadline {
                    return Err(failure(timeout_reason));
                }
            }
        }
    }
}

fn wait_ready(
    responses: &Receiver<Result<serde_json::Value, ()>>,
    control: &Control,
    started: Instant,
) -> AppResult<()> {
    let deadline = started + STARTUP_TIMEOUT;
    for expected in [
        LoadingStage::Runtime,
        LoadingStage::Models,
        LoadingStage::Warmup,
    ] {
        let progress: Loading = serde_json::from_value(wait_response(
            responses,
            control,
            deadline,
            "startup_timeout",
        )?)
        .map_err(|_| failure("startup_schema"))?;
        if progress.loading != expected {
            return Err(failure("startup_stage"));
        }
        tracing::info!(event = "lira_loading", stage = ?expected,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "live wake worker loading");
    }
    let ready: Ready = serde_json::from_value(wait_response(
        responses,
        control,
        deadline,
        "startup_timeout",
    )?)
    .map_err(|_| failure("startup_schema"))?;
    if ready.protocol != 2 || ready.cnn != CNN || ready.verifier != VERIFIER {
        return Err(failure("startup_identity"));
    }
    tracing::info!(
        event = "lira_worker_ready",
        elapsed_ms = started.elapsed().as_millis() as u64,
        "live wake models and first inference ready"
    );
    Ok(())
}
impl WarmWorker {
    pub fn start_packaged_or_dev(app: &tauri::AppHandle, control: &Control) -> AppResult<Self> {
        #[cfg(debug_assertions)]
        {
            let _ = app;
            Self::start(control)
        }
        #[cfg(not(debug_assertions))]
        {
            use tauri::Manager;
            let root = app
                .path()
                .resource_dir()
                .map_err(|_| failed())?
                .join("resources/lira");
            Self::start_bundle(&root, control)
        }
    }
    #[cfg(any(not(debug_assertions), test))]
    pub(super) fn start_bundle(root: &std::path::Path, control: &Control) -> AppResult<Self> {
        let expected = option_env!("FONO_LIRA_MANIFEST_SHA256");
        #[cfg(not(test))]
        if expected.is_none() {
            return Err(failed());
        }
        super::package::verify(root, expected).map_err(|_| failure("package_integrity"))?;
        let mut command = Command::new(root.join("runtime/python.exe"));
        command
            .arg("-I")
            .arg("-B")
            .arg(root.join("worker/worker.py"))
            .current_dir(root)
            .env_clear()
            .env("PATH", root.join("runtime"));
        for key in ["SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        Self::spawn(command, control)
    }
    #[cfg(any(debug_assertions, test))]
    pub fn start(control: &Control) -> AppResult<Self> {
        let python = std::env::var_os("FONO_WAKE_DEV_PYTHON").ok_or_else(failed)?;
        if !std::path::Path::new(&python).is_absolute() {
            return Err(failed());
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or_else(failed)?;
        let entrypoint = root.join("tools/wake-training/live_worker.py");
        #[cfg(test)]
        let entrypoint = std::env::var_os("FONO_LIRA_TEST_ENTRYPOINT")
            .map(std::path::PathBuf::from)
            .unwrap_or(entrypoint);
        let mut command = Command::new(python);
        command
            .arg(entrypoint)
            .current_dir(root)
            .env("PYTHONPATH", root.join("build/wake-training-python"))
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("PYTHONUTF8", "1");
        Self::spawn(command, control)
    }
    fn spawn(mut command: Command, control: &Control) -> AppResult<Self> {
        use std::os::windows::process::CommandExt;
        let mut child = OwnedChild(
            command
                // Interactive frame deadlines must not compete at background
                // priority with a foreground build or another busy application.
                .creation_flags(0x08000000 | 0x00000020)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| failed())?,
        );
        let job = Job::attach(&child.0)?;
        let mut input = child.0.stdin.take().ok_or_else(failed)?;
        let mut output = BufReader::new(child.0.stdout.take().ok_or_else(failed)?);
        let (tx, requests) = bounded::<Vec<u8>>(1);
        // At most three loading messages plus Ready. All fit even if Stop
        // cancels the waiter: the pipe thread must never block while dropping.
        let (responses, rx) = bounded(4);
        let io = std::thread::Builder::new()
            .name("lira-pipes".into())
            .spawn(move || {
                let mut initialized = false;
                for _ in 0..4 {
                    let reply = read_reply(&mut output);
                    let valid = reply.is_ok();
                    let ready = reply
                        .as_ref()
                        .is_ok_and(|value| value.get("protocol").is_some());
                    if responses.send(reply).is_err() || !valid {
                        return;
                    }
                    if ready {
                        initialized = true;
                        break;
                    }
                }
                if !initialized {
                    return;
                }
                while let Ok(request) = requests.recv() {
                    let reply = input
                        .write_all(&request)
                        .and_then(|_| input.flush())
                        .map_err(|_| ())
                        .and_then(|_| read_reply(&mut output));
                    let valid = reply.is_ok();
                    if responses.send(reply).is_err() || !valid {
                        break;
                    }
                }
            })
            .map_err(|_| failed())?;
        let worker = Self {
            child,
            job: Some(job),
            requests: Some(tx),
            responses: rx,
            io: Some(io),
            processed: 0,
        };
        wait_ready(&worker.responses, control, Instant::now())?;
        Ok(worker)
    }
    fn wait(
        &self,
        control: &Control,
        timeout_reason: &'static str,
    ) -> AppResult<serde_json::Value> {
        wait_response(
            &self.responses,
            control,
            Instant::now() + Duration::from_secs(5),
            timeout_reason,
        )
    }
    fn request(
        &self,
        request: Vec<u8>,
        control: &Control,
        timeout_reason: &'static str,
    ) -> AppResult<Reply> {
        self.requests
            .as_ref()
            .ok_or_else(failed)?
            .try_send(request)
            .map_err(|_| failure("request_queue"))?;
        serde_json::from_value(self.wait(control, timeout_reason)?)
            .map_err(|_| failure("reply_schema"))
    }
    pub fn reset(&mut self, control: &Control) -> AppResult<()> {
        let reply = self.request(vec![0; 5], control, "reset_timeout")?;
        if reply.processed != 0 || reply.detected {
            return Err(failure("reset_clock"));
        }
        self.processed = 0;
        Ok(())
    }
    pub fn feed(&mut self, pcm: &[i16], speech: bool, control: &Control) -> AppResult<bool> {
        if pcm.len() != FRAME {
            return Err(failed());
        }
        let mut request = Vec::with_capacity(FRAME * 2 + 6);
        request.push(1);
        request.extend_from_slice(&(FRAME as u32).to_le_bytes());
        request.push(u8::from(speech));
        for value in pcm {
            request.extend_from_slice(&value.to_le_bytes());
        }
        let reply = self.request(request, control, "frame_timeout")?;
        self.processed += FRAME as u64;
        if reply.processed != self.processed {
            return Err(failure("frame_clock"));
        }
        Ok(reply.detected)
    }
}
impl Drop for WarmWorker {
    fn drop(&mut self) {
        self.requests.take();
        self.job.take();
        let _ = self.child.0.kill();
        let _ = self.child.0.wait();
        // Closing the child pipes interrupts a blocked reply read before joining.
        if let Some(io) = self.io.take() {
            let _ = io.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn startup_messages(protocol: u32) -> [serde_json::Value; 4] {
        [
            serde_json::json!({"loading":"runtime"}),
            serde_json::json!({"loading":"models"}),
            serde_json::json!({"loading":"warmup"}),
            serde_json::json!({"protocol":protocol,"cnn":CNN,"verifier":VERIFIER}),
        ]
    }
    #[test]
    fn cold_loading_can_finish_after_one_minute_without_restarting_the_worker() {
        let (send, receive) = bounded(4);
        for message in startup_messages(2) {
            send.send(Ok(message)).unwrap();
        }
        wait_ready(
            &receive,
            &Control::new(),
            Instant::now() - Duration::from_secs(70),
        )
        .unwrap();
        assert!(receive.is_empty());
    }
    #[test]
    fn repeated_or_missing_loading_stages_and_old_protocol_are_not_ready() {
        for messages in [
            vec![serde_json::json!({"loading":"models"})],
            vec![
                serde_json::json!({"loading":"runtime"}),
                serde_json::json!({"loading":"runtime"}),
            ],
            startup_messages(1).to_vec(),
            vec![serde_json::json!({"loading":"runtime","extra":true})],
        ] {
            let (send, receive) = bounded(4);
            for message in messages {
                send.send(Ok(message)).unwrap();
            }
            assert!(wait_ready(&receive, &Control::new(), Instant::now()).is_err());
        }
    }
    #[test]
    fn silent_startup_still_expires_and_stop_wins_over_ready_models() {
        let (send, receive) = bounded(4);
        assert!(wait_ready(&receive, &Control::new(), Instant::now() - STARTUP_TIMEOUT,).is_err());
        for message in startup_messages(2) {
            send.send(Ok(message)).unwrap();
        }
        let stopped = Control::new();
        stopped.cancelled.store(true, Ordering::Release);
        assert!(matches!(
            wait_ready(&receive, &stopped, Instant::now()),
            Err(AppError::Cancelled(_))
        ));
        assert_eq!(receive.len(), 4);
    }
    #[test]
    fn scheduling_pause_does_not_discard_an_already_queued_reply() {
        let (send, receive) = bounded(1);
        let value = serde_json::json!({"processed":1280,"detected":false});
        send.send(Ok(value.clone())).unwrap();
        let expired = Instant::now() - Duration::from_secs(1);
        assert_eq!(
            wait_response(&receive, &Control::new(), expired, "test_timeout").unwrap(),
            value
        );
    }
    #[test]
    fn expired_empty_pipe_still_times_out_and_stop_wins_over_a_queued_reply() {
        let (send, receive) = bounded(1);
        let expired = Instant::now() - Duration::from_secs(1);
        assert!(matches!(
            wait_response(&receive, &Control::new(), expired, "test_timeout"),
            Err(AppError::Internal(_))
        ));
        send.send(Ok(serde_json::json!({"processed":1280,"detected":true})))
            .unwrap();
        let stopped = Control::new();
        stopped.cancelled.store(true, Ordering::Release);
        assert!(matches!(
            wait_response(&receive, &stopped, expired, "test_timeout"),
            Err(AppError::Cancelled(_))
        ));
        assert_eq!(receive.len(), 1);
    }
    #[test]
    fn replies_are_bounded_and_cannot_hide_clock_or_protocol_changes() {
        assert!(read_reply(&mut std::io::Cursor::new(vec![b'x'; 4097])).is_err());
        assert!(read_reply(&mut std::io::Cursor::new(b"{}".to_vec())).is_err());
        let value = read_reply(&mut std::io::Cursor::new(
            b"{\"processed\":1280,\"detected\":false}\n".to_vec(),
        ))
        .unwrap();
        let reply: Reply = serde_json::from_value(value).unwrap();
        assert_eq!(reply.processed, 1280);
        assert!(!reply.detected);
        assert!(serde_json::from_str::<Reply>(
            r#"{"processed":0,"detected":false,"error":"failed"}"#
        )
        .is_err());
    }
}

#[cfg(test)]
#[path = "worker_replay.rs"]
mod replay_tests;
