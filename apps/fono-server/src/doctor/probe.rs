use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use fono_core::OperationCancellation;
use fono_stt::{ModelPool, ModelResidency, SttEnvironment};
use serde::{Deserialize, Serialize};

use crate::{backends, config::ServerConfig, jobs::acceleration_for_device, models};

const ENGLISH_PCM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/doctor-en.pcm"
));
const RUSSIAN_PCM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/doctor-ru.pcm"
));

#[derive(Debug, Serialize)]
pub struct ProbeResult {
    pub status: &'static str,
    pub detail: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
}

impl ProbeResult {
    pub(super) fn skipped() -> Self {
        Self {
            status: "skipped",
            detail: "Recognition probe has not run",
            model: None,
            device: None,
            audio_ms: None,
            elapsed_ms: None,
        }
    }
    fn failed(detail: &'static str) -> Self {
        Self {
            status: "failed",
            detail,
            ..Self::skipped()
        }
    }
}

/// Private child protocol: never add decoded text or native error strings.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeReply {
    pub protocol_version: u16,
    pub passed: bool,
    pub model: String,
    pub device: String,
    pub audio_ms: u64,
    pub elapsed_ms: u64,
}

pub(super) fn run_child(
    config: &ServerConfig,
    config_path: &Path,
    timeout: Duration,
) -> ProbeResult {
    let Ok(executable) = std::env::current_exe() else {
        return ProbeResult::failed("Cannot locate the diagnostic executable");
    };
    let mut command = Command::new(executable);
    command.arg("doctor-probe").arg("--config").arg(config_path);
    let reply = match run_command(command, timeout) {
        Ok(reply) => reply,
        Err(detail) => return ProbeResult::failed(detail),
    };
    let device = crate::backends::device_for(config, &config.models.default);
    if reply.protocol_version != fono_stt_protocol::PROTOCOL_VERSION
        || reply.model != config.models.default
        || reply.device != device
    {
        return ProbeResult::failed("Native probe returned an incompatible result");
    }
    ProbeResult {
        status: if reply.passed { "passed" } else { "failed" },
        detail: if reply.passed {
            "Embedded speech recognized on the configured backend"
        } else {
            "Model load or embedded speech recognition failed"
        },
        model: Some(reply.model),
        device: Some(reply.device),
        audio_ms: Some(reply.audio_ms),
        elapsed_ms: Some(reply.elapsed_ms),
    }
}

fn run_command(mut command: Command, timeout: Duration) -> Result<ProbeReply, &'static str> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = command
        .spawn()
        .map_err(|_| "Cannot start the native diagnostic child")?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Err("Native diagnostic child exited abnormally");
                }
                break;
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                terminate(&mut child);
                return Err("Native recognition probe timed out; its process group was terminated");
            }
            Err(_) => {
                terminate(&mut child);
                return Err("Cannot supervise the native diagnostic child");
            }
        }
    }
    let mut output = Vec::new();
    child
        .stdout
        .take()
        .ok_or("Native probe output is unavailable")?
        .take(8193)
        .read_to_end(&mut output)
        .map_err(|_| "Cannot read native probe result")?;
    if output.len() > 8192 {
        return Err("Native probe exceeded its result size limit");
    }
    serde_json::from_slice(&output).map_err(|_| "Native probe returned an invalid result")
}

fn terminate(child: &mut Child) {
    #[cfg(unix)]
    {
        // SAFETY: the still-owned child was spawned as the leader of its own
        // process group. This also stops its supervised CUDA worker.
        if let Ok(pid) = i32::try_from(child.id()) {
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Run only in the private child. No token, HTTP, history or microphone I/O.
pub fn run_native_probe(config: &ServerConfig) -> ProbeReply {
    let start = Instant::now();
    let resolved = crate::backends::resolve_auto_config(config.clone());
    let config = resolved.as_ref().unwrap_or(config);
    let planned_device = crate::backends::device_for(config, &config.models.default).to_owned();
    let mut reply = ProbeReply {
        protocol_version: fono_stt_protocol::PROTOCOL_VERSION,
        passed: false,
        model: config.models.default.clone(),
        device: planned_device,
        audio_ms: 0,
        elapsed_ms: 0,
    };
    let result: Result<bool, ()> = (|| {
        if resolved.is_err() {
            return Err(());
        }
        let plan = models::resolve_installed(config, &config.models.default).map_err(|_| ())?;
        let acceleration = acceleration_for_device(&plan.device).map_err(|_| ())?;
        let russian = if plan.selection.id.engine == fono_types::SttEngine::Whisper {
            false
        } else {
            !fono_stt::offline_models::specification(&plan.selection.id)
                .map_err(|_| ())?
                .languages
                .iter()
                .any(|value| value == "en")
        };
        let (pcm, language) = if russian {
            (RUSSIAN_PCM, "ru")
        } else {
            (ENGLISH_PCM, "en")
        };
        let samples: Vec<_> = pcm
            .chunks_exact(2)
            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
            .collect();
        reply.audio_ms = (samples.len() as u64) * 1000 / 16_000;
        let pool = ModelPool::new(
            SttEnvironment {
                models_dir: config.models.dir.clone(),
                workers: backends::worker_paths(config).map_err(|_| ())?,
                ..SttEnvironment::default()
            },
            1,
        )
        .map_err(|_| ())?;
        let probe_result: Result<bool, ()> = (|| {
            let cancellation = OperationCancellation::default();
            let lease = pool
                .acquire_on_device_verified(
                    &plan.selection,
                    acceleration,
                    backends::cuda_index(&plan.device),
                    ModelResidency::Resident,
                    0,
                    &cancellation,
                    || Ok(()),
                )
                .map_err(|_| ())?;
            let transcript = lease
                .transcribe_cancellable(&samples, language, cancellation)
                .map_err(|_| ())?;
            let actual = backends::reported_device(&lease.device(), &plan.device);
            Ok(actual == plan.device && transcript_matches(&transcript.text, russian))
        })();
        pool.begin_shutdown();
        // The parent enforces the wall-clock deadline and kills this entire
        // process group if native inference or worker cleanup does not return.
        while !pool.shutdown_complete() {
            pool.reap_shutdown();
            std::thread::sleep(Duration::from_millis(10));
        }
        probe_result
    })();
    reply.passed = result.unwrap_or(false);
    reply.elapsed_ms = start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    reply
}

fn transcript_matches(text: &str, russian: bool) -> bool {
    let normalized = text.to_lowercase();
    if russian {
        normalized.contains("похвал") && normalized.contains("счастл")
    } else {
        normalized.contains("country")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    #[test]
    fn embedded_fixtures_are_pinned_pcm_and_cover_supported_languages() {
        for (bytes, digest, samples) in [
            (
                ENGLISH_PCM,
                "a29462b8ebd467318000e683b9117ade46230d3255ed2024e7db894abd9b38c9",
                176_000,
            ),
            (
                RUSSIAN_PCM,
                "539bf24c83e1b54fbb0e1d8102f5b76b1ea00dc197c3d2b64a2ea793a7e8c4a8",
                180_640,
            ),
        ] {
            assert_eq!(bytes.len(), samples * 2);
            assert_eq!(format!("{:x}", Sha256::digest(bytes)), digest);
        }
        assert!(transcript_matches("Ask what your COUNTRY can do", false));
        assert!(transcript_matches("Не требуя похвал, я счастлив", true));
        assert!(!transcript_matches("", false));
        assert!(!transcript_matches("[silence]", true));
    }
    #[cfg(unix)]
    #[test]
    fn native_deadline_terminates_the_child_instead_of_waiting_for_sleep() {
        let directory =
            std::env::temp_dir().join(format!("fono-doctor-deadline-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let pid_file = directory.join("owned-descendant.pid");
        let mut command = Command::new("sh");
        command
            .args([
                "-c",
                "sleep 10 & printf '%s' \"$!\" > \"$1\"; wait",
                "doctor-test",
            ])
            .arg(&pid_file);
        let started = Instant::now();
        assert!(run_command(command, Duration::from_secs(1))
            .unwrap_err()
            .contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(3));
        let pid: u32 = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();
        let reaping_deadline = Instant::now() + Duration::from_secs(2);
        while let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            if stat.rsplit_once(')').unwrap().1.split_whitespace().next() == Some("Z") {
                break;
            }
            assert!(
                Instant::now() < reaping_deadline,
                "owned descendant survived the deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn unexpected_child_output_is_not_forwarded_to_the_report() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf secret-native-error"]);
        let error = run_command(command, Duration::from_secs(3)).unwrap_err();
        assert_eq!(error, "Native probe returned an invalid result");
        assert!(!error.contains("secret"));
    }
}
