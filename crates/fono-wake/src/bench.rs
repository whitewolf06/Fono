//! File-only research trace. No AudioHub, microphone, IPC or application state.
use crate::backend::{decoder_session::DecoderSession, sherpa_detector::SherpaDetector};
use crate::{WakeWordBackend, WakeWordConfig, WakeWordError, WakeWordResult};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchRequest {
    pub backend: String,
    pub model_dir: PathBuf,
    pub wav: PathBuf,
    pub phrase: String,
    pub stability_ms: u64,
    pub confirmations: u8,
    pub cooldown_ms: u64,
    pub realtime: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct BenchDetection {
    /// Sample clock at delivery, independent of the decoder's token timestamp.
    pub audio_ms: f64,
    /// Wall time since playback start; meaningful for latency only in paced mode.
    pub delivered_ms: f64,
}

#[derive(serde::Serialize)]
pub struct BenchTrace {
    pub schema: u8,
    pub sample_rate: u32,
    pub audio_samples: usize,
    pub audio_duration_ms: f64,
    pub model_load_ms: f64,
    pub replay_wall_ms: f64,
    pub process_cpu_ms: Option<f64>,
    pub realtime: bool,
    pub events: Vec<BenchDetection>,
}

fn config(request: &BenchRequest) -> WakeWordResult<WakeWordConfig> {
    let backend = match (request.backend.as_str(), request.phrase.as_str()) {
        ("ru", "эй фоно") => WakeWordBackend::SherpaStreamingRu,
        ("en", "hey fono") => WakeWordBackend::SherpaStreamingEn,
        _ => {
            return Err(WakeWordError::Backend(
                "bench expects fixed RU/EN phrase".into(),
            ))
        }
    };
    if request.stability_ms > 3000
        || !(2..=10).contains(&request.confirmations)
        || request.cooldown_ms > 10_000
    {
        return Err(WakeWordError::Backend(
            "invalid bench operating point".into(),
        ));
    }
    Ok(WakeWordConfig {
        backend,
        model_dir: request.model_dir.clone(),
        phrase: request.phrase.clone(),
        phrase_stability_ms: request.stability_ms,
        phrase_confirmations: request.confirmations,
        cooldown_ms: request.cooldown_ms,
        ..Default::default()
    })
}

pub fn run(request: &BenchRequest) -> WakeWordResult<BenchTrace> {
    let mut config = config(request)?;
    // Long recordings should be split without changing their source/speaker group.
    // Bound the allocation made by Sherpa's Wave reader, never truncate audio.
    if std::fs::metadata(&request.wav)
        .map_err(|e| WakeWordError::Audio(e.to_string()))?
        .len()
        > 64 * 1024 * 1024
    {
        return Err(WakeWordError::Audio(
            "bench WAV exceeds 64 MiB; split the recording".into(),
        ));
    }
    let wave = sherpa_onnx::Wave::read(&request.wav.to_string_lossy())
        .ok_or_else(|| WakeWordError::Audio("could not read bench WAV".into()))?;
    let rate = wave.sample_rate();
    if rate != 16_000 || wave.samples().is_empty() {
        return Err(WakeWordError::Audio(
            "bench expects nonempty mono 16 kHz WAV".into(),
        ));
    }
    config.sample_rate = rate as u32;
    crate::validate_config(&config)?;
    let loading = Instant::now();
    let mut detector = DecoderSession::new(SherpaDetector::new(&config)?, config.sample_rate);
    let model_load_ms = loading.elapsed().as_secs_f64() * 1000.0;
    let cpu_start = process_cpu_ms();
    let started = Instant::now();
    let mut events = Vec::new();
    let mut clock = 0_u64;
    let mut cooldown_until = 0_u64;
    for chunk in wave.samples().chunks(320) {
        clock += chunk.len() as u64;
        if request.realtime {
            let deadline = started + Duration::from_secs_f64(clock as f64 / rate as f64);
            if let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
                std::thread::sleep(remaining);
            }
        }
        if detector.accept(chunk, clock).is_some() && clock >= cooldown_until {
            events.push(BenchDetection {
                audio_ms: clock as f64 * 1000.0 / rate as f64,
                delivered_ms: started.elapsed().as_secs_f64() * 1000.0,
            });
            cooldown_until = clock + config.cooldown_ms * rate as u64 / 1000;
        }
    }
    if detector.finish(clock).is_some() && clock >= cooldown_until {
        events.push(BenchDetection {
            audio_ms: clock as f64 * 1000.0 / rate as f64,
            delivered_ms: started.elapsed().as_secs_f64() * 1000.0,
        });
    }
    let replay_wall_ms = started.elapsed().as_secs_f64() * 1000.0;
    let cpu_end = process_cpu_ms();
    Ok(BenchTrace {
        schema: 1,
        sample_rate: config.sample_rate,
        audio_samples: wave.samples().len(),
        audio_duration_ms: clock as f64 * 1000.0 / rate as f64,
        model_load_ms,
        replay_wall_ms,
        process_cpu_ms: cpu_start.zip(cpu_end).map(|(a, b)| b - a),
        realtime: request.realtime,
        events,
    })
}

#[cfg(windows)]
fn process_cpu_ms() -> Option<f64> {
    use std::ffi::c_void;
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn GetProcessTimes(
            process: *mut c_void,
            created: *mut FileTime,
            exited: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }
    let (mut created, mut exited, mut kernel, mut user) = (
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
        FileTime::default(),
    );
    // The pseudo-handle refers only to this standalone process and is not closed.
    let ok = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    let ticks = |t: FileTime| ((t.high as u64) << 32 | t.low as u64) as f64;
    (ok != 0).then(|| (ticks(kernel) + ticks(user)) / 10_000.0)
}

#[cfg(not(windows))]
fn process_cpu_ms() -> Option<f64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> BenchRequest {
        BenchRequest {
            backend: "ru".into(),
            model_dir: "unused".into(),
            wav: "unused".into(),
            phrase: "эй фоно".into(),
            stability_ms: 250,
            confirmations: 2,
            cooldown_ms: 2000,
            realtime: false,
        }
    }
    #[test]
    fn bench_rejects_retired_or_arbitrary_targets_before_file_io() {
        for (backend, phrase) in [
            ("kws", "hey fono"),
            ("ru", "hello computer"),
            ("en", "эй фоно"),
        ] {
            let mut value = request();
            value.backend = backend.into();
            value.phrase = phrase.into();
            assert!(config(&value).is_err());
        }
    }
    #[test]
    fn bench_bounds_points_and_preserves_current_baseline() {
        let mut value = request();
        let baseline = config(&value).unwrap();
        assert_eq!(baseline.phrase_stability_ms, 250);
        assert_eq!(baseline.phrase_confirmations, 2);
        assert_eq!(baseline.cooldown_ms, 2000);
        value.confirmations = 1;
        assert!(config(&value).is_err());
        value.confirmations = 2;
        value.stability_ms = 3001;
        assert!(config(&value).is_err());
    }
}
