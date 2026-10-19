//! Explicit public-corpus measurement through the production recognizers.
//! Never opens app settings, a microphone, a window or external input.
use super::{SttEngine, WorkerPaths};
use crate::{
    application::audio_ingest::{decode_file, AudioIngestPolicy},
    types::{AccelerationMode, SttModelSelection},
};
use fono_audio::speech_segments::{segment_recording, SpeechSegment, DEFAULT_MAX_SEGMENT_SAMPLES};
use fono_core::OperationCancellation;
use fono_stt_protocol::{BackendKind, DecodingOptions};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

struct Prepared {
    id: String,
    language: String,
    samples: Vec<i16>,
    segments: Vec<SpeechSegment>,
    has_speech: bool,
}
fn recognize(engine: &SttEngine, audio: &Prepared, operation: u64) -> (String, f64) {
    let started = Instant::now();
    if !audio.has_speech {
        return (String::new(), 0.0);
    }
    let cancellation = OperationCancellation::default();
    let result = crate::application::segmented_transcription::transcribe_segments(
        &audio.samples,
        &audio.segments,
        &cancellation,
        |segment, start| {
            (
                engine.transcribe_window_with_options(
                    segment,
                    &audio.language,
                    None,
                    operation,
                    &cancellation,
                    start,
                    if engine.capabilities().unwrap().decoding_options {
                        DecodingOptions {
                            threads: Some(8),
                            ..DecodingOptions::default()
                        }
                    } else {
                        DecodingOptions::default()
                    },
                ),
                false,
            )
        },
    )
    .unwrap();
    assert_eq!(result.backend, BackendKind::Cpu);
    (result.text, started.elapsed().as_secs_f64())
}
fn save(output: &Path, report: &Value) {
    std::fs::write(output, serde_json::to_vec_pretty(report).unwrap()).unwrap();
}

#[test]
#[ignore = "explicit FONO-119 public-corpus CPU comparison; needs pinned models"]
fn public_corpus_model_comparison() {
    let selection: SttModelSelection = serde_json::from_str(
        &std::env::var("FONO_COMPARISON_SELECTION").expect("explicit model selection required"),
    )
    .unwrap();
    selection.validate().unwrap();
    let output = PathBuf::from(
        std::env::var("FONO_COMPARISON_OUTPUT").expect("explicit report path required"),
    );
    assert!(output.is_absolute());
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let corpus_dir = root.join("tests/fixtures/stt-quality");
    let corpus_bytes = std::fs::read(corpus_dir.join("manifest.json")).unwrap();
    let manifest: Value = serde_json::from_slice(&corpus_bytes).unwrap();
    assert_eq!(manifest["schema"], 1);
    assert_eq!(manifest["entries"].as_array().unwrap().len(), 20);
    let vad_path = root.join("resources/vad/silero_vad.onnx");
    let mut gate = fono_audio::StreamingSpeechVad::new(&vad_path).unwrap();
    let mut prepared = Vec::new();
    let mut preprocessing = Vec::new();
    for entry in manifest["entries"].as_array().unwrap() {
        let relative = entry["path"].as_str().unwrap();
        assert_eq!(Path::new(relative).file_name().unwrap(), relative);
        let path = corpus_dir.join(relative);
        assert_eq!(
            format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap())),
            entry["sha256"]
        );
        let started = Instant::now();
        let audio = decode_file(&path, AudioIngestPolicy::default()).unwrap();
        assert_eq!(
            (audio.source_sample_rate, audio.source_channels),
            (16000, 1)
        );
        let decision = gate
            .analyze_recording(&audio.pcm_samples, || false)
            .unwrap();
        let (samples, _) = crate::vad::prepare_recording_with_result(&audio.pcm_samples, &decision);
        let segments =
            segment_recording(&samples, &vad_path, DEFAULT_MAX_SEGMENT_SAMPLES, || false)
                .unwrap()
                .unwrap();
        let mut digest = Sha256::new();
        for sample in &samples {
            digest.update(sample.to_le_bytes());
        }
        preprocessing.push(json!({"id":entry["id"],"source_sha256":entry["sha256"],"pcm_sha256":format!("{:x}",digest.finalize()),
            "samples":samples.len(),"has_speech":decision.has_speech,"seconds":started.elapsed().as_secs_f64(),
            "segments":segments.iter().map(|s|json!({"start":s.start,"end":s.end,"forced":s.forced})).collect::<Vec<_>>()}));
        prepared.push(Prepared {
            id: entry["id"].as_str().unwrap().into(),
            language: entry["language"].as_str().unwrap().into(),
            samples,
            segments,
            has_speech: decision.has_speech,
        });
    }
    let languages = if selection.id.engine == crate::types::SttEngine::Whisper {
        vec!["ru".into(), "en".into()]
    } else {
        super::offline_models::specification(&selection.id)
            .unwrap()
            .languages
    };
    let mut report = json!({"schema":1,"model":selection.id,"device":"CPU","profile":if cfg!(debug_assertions){"debug"}else{"release"},
        "corpus_sha256":format!("{:x}",Sha256::digest(&corpus_bytes)),"vad_sha256":format!("{:x}",Sha256::digest(std::fs::read(&vad_path).unwrap())),
        "policy":{"preprocessing":"production decode_file + Silero admission + full-PCM preparation + shared 25s segmentation",
            "whisper_threads":8,"whisper_strategy":"fast","sherpa_threads":2,"sherpa_decoding":"greedy_search"},
        "preprocessing":preprocessing,"entries":[],"completed":false});
    save(&output, &report);
    let engine = SttEngine::new();
    let started = Instant::now();
    engine
        .ensure_model_loaded(
            &selection,
            AccelerationMode::Cpu,
            &WorkerPaths::default().with_speech_vad(vad_path),
            &OperationCancellation::default(),
        )
        .unwrap();
    assert_eq!(engine.device(), "CPU");
    assert_eq!(engine.model(), Some(selection.id.clone()));
    report["model_load_seconds"] = json!(started.elapsed().as_secs_f64());
    let cold = prepared.iter().find(|audio| audio.id == "ru-01").unwrap();
    let (text, seconds) = recognize(&engine, cold, 1);
    report["cold"] = json!({"id":cold.id,"text":text,"seconds":seconds});
    save(&output, &report);
    for (index, audio) in prepared.iter().enumerate() {
        let entry = if languages.contains(&audio.language) {
            eprintln!("comparison {}: {}", selection.id.public_id(), audio.id);
            let (text, seconds) = recognize(&engine, audio, index as u64 + 2);
            json!({"id":audio.id,"language":audio.language,"status":"completed","text":text,
                "warm_seconds":seconds,"audio_seconds":audio.samples.len() as f64/16000.0})
        } else {
            json!({"id":audio.id,"language":audio.language,"status":"unsupported"})
        };
        report["entries"].as_array_mut().unwrap().push(entry);
        save(&output, &report);
    }
    report["completed"] = json!(true);
    save(&output, &report);
}
