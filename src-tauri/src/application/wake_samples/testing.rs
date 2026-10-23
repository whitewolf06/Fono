//! Explicit, bounded file verification for owner dev checks; never a live wake backend.
use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};

static BUSY: AtomicBool = AtomicBool::new(false);
#[cfg(all(windows, debug_assertions))]
pub(crate) fn is_busy() -> bool {
    BUSY.load(Ordering::Acquire)
}
const UNAVAILABLE: &str = "Проверка детектора недоступна в этой сборке.";
#[cfg(any(debug_assertions, test))]
const FAILED: &str = "Не удалось проверить запись. Повторите проверку.";

pub(crate) fn available() -> bool {
    cfg!(debug_assertions)
        && std::env::var_os("FONO_WAKE_DEV_PYTHON")
            .is_some_and(|path| std::path::Path::new(&path).is_absolute())
}
pub(crate) struct Reservation {
    pub operation_id: String,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}
pub(crate) fn reserve() -> AppResult<Reservation> {
    if !available() {
        return Err(AppError::Config(UNAVAILABLE.into()));
    }
    BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| AppError::Busy("Дождитесь завершения проверки записи.".into()))?;
    Ok(Reservation {
        operation_id: uuid::Uuid::new_v4().to_string(),
    })
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Report {
    pub detected: bool,
    pub proposals: u32,
    pub events: u32,
    pub processing_ms: u64,
    pub duration_ms: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Result {
    pub operation_id: String,
    #[serde(flatten)]
    pub report: Report,
}
#[cfg(debug_assertions)]
pub(crate) fn run(id: &str, reservation: Reservation) -> AppResult<Result> {
    let bytes = super::storage::detector_input(id)?;
    let python = std::env::var_os("FONO_WAKE_DEV_PYTHON")
        .ok_or_else(|| AppError::Config(UNAVAILABLE.into()))?;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| AppError::Config(UNAVAILABLE.into()))?;
    let mut command = std::process::Command::new(python);
    command
        .arg(root.join("tools/wake-training/dev_check.py"))
        .current_dir(root)
        .env("PYTHONPATH", root.join("build/wake-training-python"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PYTHONUTF8", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000 | 0x00004000); // Hidden, below normal priority.
    }
    let child = command
        .spawn()
        .map_err(|_| AppError::Internal(FAILED.into()))?;
    let report = collect(child, bytes, std::time::Duration::from_secs(60))?;
    Ok(Result {
        operation_id: reservation.operation_id.clone(),
        report,
    })
}
#[cfg(not(debug_assertions))]
pub(crate) fn run(_id: &str, _reservation: Reservation) -> AppResult<Result> {
    Err(AppError::Config(UNAVAILABLE.into()))
}

#[cfg(any(debug_assertions, test))]
fn collect(
    child: std::process::Child,
    bytes: Vec<u8>,
    timeout: std::time::Duration,
) -> AppResult<Report> {
    use std::io::{Read, Write};
    // The child, writer and update lease survive cancellation of the IPC waiter.
    struct Owned(std::process::Child);
    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Owned(child);
    let mut stdin = child
        .0
        .stdin
        .take()
        .ok_or_else(|| AppError::Internal(FAILED.into()))?;
    // A stalled interpreter must not block the timeout while receiving PCM.
    let writer = std::thread::spawn(move || stdin.write_all(&bytes));
    let started = std::time::Instant::now();
    let status = loop {
        match child.0.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(std::time::Duration::from_millis(25))
            }
            _ => {
                let _ = child.0.kill();
                let _ = child.0.wait();
                let _ = writer.join();
                return Err(AppError::Internal(FAILED.into()));
            }
        }
    };
    writer
        .join()
        .map_err(|_| AppError::Internal(FAILED.into()))??;
    if !status.success() {
        return Err(AppError::Internal(FAILED.into()));
    }
    let mut output = Vec::new();
    child
        .0
        .stdout
        .take()
        .ok_or_else(|| AppError::Internal(FAILED.into()))?
        .take(4097)
        .read_to_end(&mut output)?;
    parse(&output)
}
#[cfg(any(debug_assertions, test))]
fn parse(output: &[u8]) -> AppResult<Report> {
    let report: Report =
        serde_json::from_slice(output).map_err(|_| AppError::Internal(FAILED.into()))?;
    if output.len() > 4096
        || report.events > report.proposals
        || report.proposals > 12
        || report.detected != (report.events > 0)
        || !(250..=22_000).contains(&report.duration_ms)
        || report.processing_ms > 60_000
    {
        return Err(AppError::Internal(FAILED.into()));
    }
    Ok(report)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn stalled_worker_with_unread_pcm_is_killed_and_joined_by_timeout() {
        use std::os::windows::process::CommandExt;
        let child = std::process::Command::new("powershell.exe")
            .creation_flags(0x08000000)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 30",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        assert!(collect(
            child,
            vec![0; 704_044],
            std::time::Duration::from_millis(100)
        )
        .is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }
    #[test]
    fn failed_or_malformed_worker_output_cannot_become_a_successful_negative() {
        assert!(parse(
            br#"{"detected":false,"proposals":2,"events":1,"processingMs":1,"durationMs":5000}"#
        )
        .is_err());
        assert!(parse(
            br#"{"detected":true,"proposals":0,"events":1,"processingMs":1,"durationMs":5000}"#
        )
        .is_err());
        assert!(parse(
            br#"{"detected":false,"proposals":0,"events":0,"processingMs":1,"durationMs":0}"#
        )
        .is_err());
        assert!(parse(b"worker failed").is_err());
        assert!(parse(
            br#"{"detected":false,"proposals":0,"events":0,"processingMs":1,"durationMs":5000}"#
        )
        .is_ok());
    }
}
