use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use base64::Engine;
use fono_stt_protocol::{BackendKind, WorkerRequest, WorkerResponse};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let mut loaded: Option<(String, Arc<WhisperContext>)> = None;

    for line in stdin.lock().lines() {
        let response = match line {
            Ok(line) => handle_line(&line, &mut loaded),
            Err(error) => WorkerResponse::Error {
                id: None,
                code: "stdin".into(),
                message: error.to_string(),
            },
        };
        let _ = writeln!(stdout, "{}", serde_json::to_string(&response).unwrap());
        let _ = stdout.flush();
    }
}

fn handle_line(
    line: &str,
    loaded: &mut Option<(String, Arc<WhisperContext>)>,
) -> WorkerResponse {
    let request = match serde_json::from_str::<WorkerRequest>(line) {
        Ok(request) => request,
        Err(error) => {
            return WorkerResponse::Error {
                id: None,
                code: "request_json".into(),
                message: error.to_string(),
            }
        }
    };
    match request {
        WorkerRequest::Ping => WorkerResponse::Ready { backend: backend() },
        WorkerRequest::Transcribe {
            id,
            model_path,
            language,
            samples_i16_base64,
        } => transcribe(id, &model_path, &language, &samples_i16_base64, loaded),
    }
}

fn transcribe(
    id: String,
    model_path: &str,
    language: &str,
    samples_i16_base64: &str,
    loaded: &mut Option<(String, Arc<WhisperContext>)>,
) -> WorkerResponse {
    let samples = match decode_i16(samples_i16_base64) {
        Ok(samples) => samples,
        Err(message) => return worker_error(Some(id), "audio", message),
    };
    let context = match load_context(model_path, loaded) {
        Ok(context) => context,
        Err(message) => return worker_error(Some(id), "model_load", message),
    };
    let started = Instant::now();
    let pcm: Vec<f32> = samples
        .iter()
        .map(|sample| *sample as f32 / i16::MAX as f32)
        .collect();
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    if !language.is_empty() && language != "auto" {
        params.set_language(Some(language));
    }
    params.set_n_threads(8);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_print_special(false);
    params.set_no_context(true);
    params.set_single_segment(true);
    params.set_no_timestamps(true);

    let mut state = match context.create_state() {
        Ok(state) => state,
        Err(error) => return worker_error(Some(id), "state", error.to_string()),
    };
    if let Err(error) = state.full(params, &pcm) {
        return worker_error(Some(id), "transcribe", error.to_string());
    }
    let text = (0..state.full_n_segments())
        .filter_map(|index| state.get_segment(index))
        .filter_map(|segment| segment.to_str_lossy().ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    WorkerResponse::Result {
        id,
        text,
        audio_secs: samples.len() as f32 / 16_000.0,
        transcribe_secs: started.elapsed().as_secs_f32(),
        backend: backend(),
    }
}

fn load_context(
    model_path: &str,
    loaded: &mut Option<(String, Arc<WhisperContext>)>,
) -> Result<Arc<WhisperContext>, String> {
    if let Some((loaded_path, context)) = loaded.as_ref() {
        if loaded_path == model_path {
            return Ok(context.clone());
        }
    }
    if !Path::new(model_path).exists() {
        return Err(format!("model not found: {model_path}"));
    }
    let mut params = WhisperContextParameters::default();
    params.use_gpu(!matches!(backend(), BackendKind::Cpu));
    let context = Arc::new(
        WhisperContext::new_with_params(model_path, params).map_err(|error| error.to_string())?,
    );
    *loaded = Some((model_path.to_string(), context.clone()));
    Ok(context)
}

fn decode_i16(value: &str) -> Result<Vec<i16>, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|error| error.to_string())?;
    if bytes.len() % 2 != 0 {
        return Err("PCM payload has an odd byte length".into());
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
        .collect())
}

fn worker_error(id: Option<String>, code: &str, message: String) -> WorkerResponse {
    WorkerResponse::Error {
        id,
        code: code.into(),
        message,
    }
}

fn backend() -> BackendKind {
    #[cfg(feature = "cuda")]
    return BackendKind::Cuda;
    #[cfg(all(feature = "vulkan", not(feature = "cuda")))]
    return BackendKind::Vulkan;
    #[cfg(not(any(feature = "cuda", feature = "vulkan")))]
    BackendKind::Cpu
}
