use super::{require, Path, PathBuf};
use sha2::{Digest, Sha256};

pub(super) const MODEL_SHA256: &str =
    "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21";

pub(super) struct Config {
    pub worker: PathBuf,
    pub model: PathBuf,
    pub wav: PathBuf,
    pub report: PathBuf,
    pub cycles: usize,
    pub faults: usize,
    pub active_cancel_every: usize,
    pub idle_seconds: u64,
    pub runner_pid: u32,
    pub timeout_seconds: u64,
    pub worker_sha256: String,
    pub wav_sha256: String,
}
fn variable(key: &str) -> Result<String, String> {
    std::env::var(key).map_err(|_| format!("explicit {key} is required"))
}
fn count(key: &str, maximum: usize) -> Result<usize, String> {
    let raw = variable(key)?;
    let value = raw.parse::<usize>().map_err(|_| format!("invalid {key}"))?;
    require(
        value > 0 && value <= maximum,
        &format!("{key} outside 1..={maximum}"),
    )?;
    Ok(value)
}
pub(super) fn hash_file(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let length = file.read(&mut chunk).map_err(|e| e.to_string())?;
        if length == 0 {
            break;
        }
        digest.update(&chunk[..length]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn regular_absolute(key: &str, executable: bool) -> Result<PathBuf, String> {
    let path = PathBuf::from(variable(key)?);
    require(path.is_absolute(), &format!("{key} must be absolute"))?;
    let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    require(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        &format!("{key} must be a regular file"),
    )?;
    if executable {
        require(
            path.extension()
                .is_some_and(|v| v.eq_ignore_ascii_case("exe")),
            "worker must be an explicit Windows EXE",
        )?;
    }
    Ok(path)
}
impl Config {
    pub fn read() -> Result<Self, String> {
        let worker = regular_absolute("FONO_SOAK_WORKER", true)?;
        let model = regular_absolute("FONO_SOAK_MODEL", false)?;
        let wav = regular_absolute("FONO_SOAK_WAV", false)?;
        let report = PathBuf::from(variable("FONO_SOAK_REPORT")?);
        require(report.is_absolute(), "FONO_SOAK_REPORT must be absolute")?;
        require(
            !report.exists(),
            "soak report already exists; preserve prior evidence",
        )?;
        require(
            report != worker && report != model && report != wav,
            "report cannot overwrite a verified input",
        )?;
        let worker_sha256 = variable("FONO_SOAK_WORKER_SHA256")?;
        let wav_sha256 = variable("FONO_SOAK_WAV_SHA256")?;
        require(
            hash_file(&worker)? == worker_sha256,
            "CPU worker SHA256 differs from explicit expected hash",
        )?;
        require(
            hash_file(&model)? == MODEL_SHA256,
            "pinned public multilingual tiny model required",
        )?;
        require(
            hash_file(&wav)? == wav_sha256,
            "public WAV SHA256 differs from expected hash",
        )?;
        let cycles = count("FONO_SOAK_CYCLES", 100_000)?;
        let faults = count("FONO_SOAK_FAULTS", 10_000)?;
        let active_cancel_every = count("FONO_SOAK_ACTIVE_CANCEL_EVERY", cycles)?;
        let idle_seconds = variable("FONO_SOAK_IDLE_SECONDS")?
            .parse::<u64>()
            .map_err(|e| e.to_string())?;
        require(idle_seconds <= 7200, "idle exceeds bounded two-hour soak")?;
        let runner_pid = variable("FONO_SOAK_RUNNER_PID")?
            .parse::<u32>()
            .map_err(|e| e.to_string())?;
        require(
            runner_pid > 0 && runner_pid != std::process::id(),
            "external controlling runner PID required",
        )?;
        let timeout_seconds = variable("FONO_SOAK_TIMEOUT_SECONDS")?
            .parse::<u64>()
            .map_err(|e| e.to_string())?;
        require(
            timeout_seconds > idle_seconds && timeout_seconds <= 14_400,
            "native watchdog timeout outside requested bounded window",
        )?;
        Ok(Self {
            worker,
            model,
            wav,
            report,
            cycles,
            faults,
            active_cancel_every,
            idle_seconds,
            runner_pid,
            timeout_seconds,
            worker_sha256,
            wav_sha256,
        })
    }
}

/// This harness accepts only the committed corpus format, with bounded parsing.
pub(super) fn read_pcm(path: &Path) -> Result<Vec<i16>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    require(
        bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE",
        "expected RIFF WAV",
    )?;
    require(
        bytes.len() <= 4_000_000,
        "WAV exceeds harness 125-second limit",
    )?;
    let mut offset = 12;
    let mut format = false;
    let mut pcm = None;
    while offset + 8 <= bytes.len() {
        let size = u32::from_le_bytes(
            bytes[offset + 4..offset + 8]
                .try_into()
                .map_err(|_| "invalid WAV size")?,
        ) as usize;
        let start = offset + 8;
        let end = start.checked_add(size).ok_or("WAV chunk overflow")?;
        require(end <= bytes.len(), "truncated WAV chunk")?;
        match &bytes[offset..offset + 4] {
            b"fmt " => {
                require(size >= 16, "WAV format chunk too short")?;
                let u16_at =
                    |index| u16::from_le_bytes([bytes[start + index], bytes[start + index + 1]]);
                let rate = u32::from_le_bytes(
                    bytes[start + 4..start + 8]
                        .try_into()
                        .map_err(|_| "invalid WAV rate")?,
                );
                require(
                    u16_at(0) == 1 && u16_at(2) == 1 && rate == 16000 && u16_at(14) == 16,
                    "expected mono PCM16 16kHz WAV",
                )?;
                format = true;
            }
            b"data" => {
                require(
                    size > 0 && size % 2 == 0,
                    "WAV PCM must be nonempty and aligned",
                )?;
                pcm = Some(
                    bytes[start..end]
                        .chunks_exact(2)
                        .map(|b| i16::from_le_bytes([b[0], b[1]]))
                        .collect(),
                );
            }
            _ => {}
        }
        offset = end + size % 2;
    }
    require(format, "missing WAV PCM format")?;
    pcm.ok_or_else(|| "missing WAV data".into())
}
