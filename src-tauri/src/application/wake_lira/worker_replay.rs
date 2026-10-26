//! Explicit offline integration check, never part of the ordinary test suite.
use super::*;
use crate::application::audio_ingest::{decode_file, AudioIngestPolicy};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn consented_pcm(path: &Path) -> Vec<i16> {
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.with_file_name("recording.json")).unwrap())
            .unwrap();
    assert_eq!(metadata["permission"], "consented-local-only");
    assert_eq!(metadata["split"], "dev");
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(
        metadata["sample"]["sha256"],
        format!("{:x}", Sha256::digest(&bytes))
    );
    decode_file(
        path,
        AudioIngestPolicy {
            max_file_bytes: 1_000_000,
            max_duration_seconds: 22,
        },
    )
    .unwrap()
    .pcm_samples
}

#[test]
#[ignore = "explicit local models and consenting fixtures; no microphone"]
fn real_worker_rearms_in_one_process_with_native_job_and_pipe_client() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_test_writer()
        .try_init();
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            std::env::var_os("FONO_LIRA_REPLAY_MANIFEST").expect("explicit replay manifest"),
        )
        .unwrap(),
    )
    .unwrap();
    let positive = consented_pcm(&PathBuf::from(manifest["positive"].as_str().unwrap()));
    let room = consented_pcm(&PathBuf::from(manifest["room"].as_str().unwrap()));
    assert!(!room.is_empty());
    let mut positive_with_tail = positive;
    positive_with_tail.extend(room.iter().copied().cycle().take(32_000));
    let mut worker = if let Some(root) = std::env::var_os("FONO_LIRA_PACKAGE") {
        WarmWorker::start_bundle(&PathBuf::from(root), &Control::new())
    } else {
        WarmWorker::start(&Control::new())
    }
    .expect("native worker handshake");
    let control = Control::new();
    let mut vad = fono_audio::StreamingSpeechVad::new(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/vad/silero_vad.onnx"),
    )
    .expect("native neural speech gate");
    let mut rows = Vec::new();
    for pass in 0..4 {
        worker.reset(&control).expect("native reset");
        vad.reset();
        let pcm: Vec<i16> = if pass < 3 {
            positive_with_tail.clone()
        } else {
            room.iter().copied().cycle().take(480_000).collect()
        };
        let start = Instant::now();
        let (mut events, mut slowest) = (0, Duration::ZERO);
        for (index, samples) in pcm.chunks(FRAME).enumerate() {
            let mut frame = [0; FRAME];
            frame[..samples.len()].copy_from_slice(samples);
            let sent = Instant::now();
            let decision = vad.accept(&frame);
            let speech = decision.has_speech
                && decision
                    .processed_samples
                    .saturating_sub(decision.last_speech_sample.unwrap_or(0))
                    <= 64_000;
            events += usize::from(
                worker
                    .feed(&frame, speech, &control)
                    .expect("native frame reply"),
            );
            slowest = slowest.max(sent.elapsed());
            let deadline = start + Duration::from_millis((index as u64 + 1) * 80);
            if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
        }
        rows.push(
            serde_json::json!({"pass":pass,"events":events,"max_frame_ms":slowest.as_millis()}),
        );
        assert_eq!(events, usize::from(pass < 3), "fixed existing case");
    }
    println!(
        "{}",
        serde_json::json!({"same_worker":true,"microphone":false,"native_neural_vad":true,"native_job_and_pipes":true,"rows":rows})
    );
}
