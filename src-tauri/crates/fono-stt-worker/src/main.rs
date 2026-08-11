// This worker communicates exclusively through redirected stdin/stdout from
// Fono. Marking it as a GUI executable prevents Windows from flashing a
// terminal window when the helper is started.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::io::{self, BufRead, ErrorKind, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use base64::Engine;
use fono_stt_protocol::{
    BackendKind, RequestMeta, WorkerCapabilities, WorkerRequest, WorkerResponse,
    MAX_REQUEST_FRAME_BYTES, PROTOCOL_VERSION,
};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

fn main() {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut stdout = io::stdout().lock();
    let mut loaded: Option<(String, Arc<WhisperContext>)> = None;

    loop {
        let line = match read_limited_line(&mut reader, MAX_REQUEST_FRAME_BYTES) {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(error) => {
                let response = worker_error(
                    RequestMeta::new("unparsed", None),
                    "request_frame",
                    error.to_string(),
                );
                write_response(&mut stdout, &response);
                continue;
            }
        };
        let (response, shutdown) = handle_line(&line, &mut loaded);
        write_response(&mut stdout, &response);
        if shutdown {
            break;
        }
    }
}

fn write_response(stdout: &mut impl Write, response: &WorkerResponse) {
    if let Ok(json) = serde_json::to_string(response) {
        let _ = writeln!(stdout, "{json}");
        let _ = stdout.flush();
    }
}

fn handle_line(
    line: &str,
    loaded: &mut Option<(String, Arc<WhisperContext>)>,
) -> (WorkerResponse, bool) {
    let request = match serde_json::from_str::<WorkerRequest>(line) {
        Ok(request) => request,
        Err(error) => {
            return (
                worker_error(
                    RequestMeta::new("unparsed", None),
                    "request_json",
                    error.to_string(),
                ),
                false,
            )
        }
    };
    let meta = request.meta().clone();
    if meta.protocol_version != PROTOCOL_VERSION {
        return (
            worker_error(
                meta,
                "protocol_version",
                format!(
                    "unsupported protocol version {}; expected {PROTOCOL_VERSION}",
                    request.meta().protocol_version
                ),
            ),
            false,
        );
    }

    let response = match request {
        WorkerRequest::Hello { meta } => WorkerResponse::Ready {
            protocol_version: PROTOCOL_VERSION,
            request_id: meta.request_id,
            backend: backend(),
            capabilities: WorkerCapabilities::default(),
        },
        WorkerRequest::Ping { meta } => WorkerResponse::Pong {
            protocol_version: PROTOCOL_VERSION,
            request_id: meta.request_id,
            backend: backend(),
        },
        WorkerRequest::Load { meta, model_path } => match load_context(&model_path, loaded) {
            Ok(_) => WorkerResponse::ModelLoaded {
                protocol_version: PROTOCOL_VERSION,
                request_id: meta.request_id,
                backend: backend(),
            },
            Err(message) => worker_error(meta, "model_load", message),
        },
        WorkerRequest::Transcribe {
            meta,
            model_path,
            language,
            samples_i16_base64,
        } => transcribe(meta, &model_path, &language, &samples_i16_base64, loaded),
        WorkerRequest::Shutdown { meta } => {
            loaded.take();
            return (
                WorkerResponse::ShuttingDown {
                    protocol_version: PROTOCOL_VERSION,
                    request_id: meta.request_id,
                },
                true,
            );
        }
    };
    (response, false)
}

fn transcribe(
    meta: RequestMeta,
    model_path: &str,
    language: &str,
    samples_i16_base64: &str,
    loaded: &mut Option<(String, Arc<WhisperContext>)>,
) -> WorkerResponse {
    let samples = match decode_i16(samples_i16_base64) {
        Ok(samples) => samples,
        Err(message) => return worker_error(meta, "audio", message),
    };
    let context = match load_context(model_path, loaded) {
        Ok(context) => context,
        Err(message) => return worker_error(meta, "model_load", message),
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
        Err(error) => return worker_error(meta, "state", error.to_string()),
    };
    if let Err(error) = state.full(params, &pcm) {
        return worker_error(meta, "transcribe", error.to_string());
    }
    let text = (0..state.full_n_segments())
        .filter_map(|index| state.get_segment(index))
        .filter_map(|segment| segment.to_str_lossy().ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    WorkerResponse::Result {
        protocol_version: PROTOCOL_VERSION,
        request_id: meta.request_id,
        operation_id: meta.operation_id.unwrap_or_else(|| "unknown".into()),
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

fn worker_error(meta: RequestMeta, code: &str, message: String) -> WorkerResponse {
    WorkerResponse::Error {
        protocol_version: PROTOCOL_VERSION,
        request_id: meta.request_id,
        operation_id: meta.operation_id,
        code: code.into(),
        message,
    }
}

fn read_limited_line<R: BufRead>(
    reader: &mut R,
    maximum_bytes: usize,
) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let mut exceeded_limit = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if bytes.is_empty() {
                return if exceeded_limit {
                    Err(io::Error::new(
                        ErrorKind::InvalidData,
                        format!("request frame exceeds {maximum_bytes} bytes"),
                    ))
                } else {
                    Ok(None)
                };
            }
            break;
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        if !exceeded_limit && bytes.len().saturating_add(consumed) > maximum_bytes {
            exceeded_limit = true;
            bytes.clear();
        }
        if !exceeded_limit {
            bytes.extend_from_slice(&available[..consumed]);
        }
        reader.consume(consumed);
        if newline.is_some() {
            if exceeded_limit {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    format!("request frame exceeds {maximum_bytes} bytes"),
                ));
            }
            break;
        }
    }
    while matches!(bytes.last(), Some(b'\n' | b'\r')) {
        bytes.pop();
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
}

fn backend() -> BackendKind {
    #[cfg(feature = "cuda")]
    return BackendKind::Cuda;
    #[cfg(all(feature = "vulkan", not(feature = "cuda")))]
    return BackendKind::Vulkan;
    #[cfg(not(any(feature = "cuda", feature = "vulkan")))]
    BackendKind::Cpu
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn oversized_request_is_rejected_without_consuming_next_frame() {
        let mut input = Cursor::new(b"too-long\n{}\n");
        assert!(read_limited_line(&mut input, 4).is_err());
        assert_eq!(read_limited_line(&mut input, 4).unwrap(), Some("{}".into()));
    }

    #[test]
    fn mismatched_protocol_version_returns_a_structured_error() {
        let request = WorkerRequest::Ping {
            meta: RequestMeta {
                protocol_version: PROTOCOL_VERSION + 1,
                request_id: "ping-1".into(),
                operation_id: None,
            },
        };
        let (response, shutdown) =
            handle_line(&serde_json::to_string(&request).unwrap(), &mut None);
        assert!(!shutdown);
        assert!(matches!(
            response,
            WorkerResponse::Error { request_id, code, .. }
                if request_id == "ping-1" && code == "protocol_version"
        ));
    }
}
