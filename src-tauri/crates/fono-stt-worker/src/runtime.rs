use base64::Engine;
use fono_stt_protocol::{
    inference::InferenceState, BackendKind, RequestMeta, WorkerCapabilities, WorkerRequest,
    WorkerResponse, PROTOCOL_VERSION,
};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use whisper_rs::{WhisperContext, WhisperContextParameters};

pub(crate) type Loaded = Option<(String, InferenceState)>;

pub(crate) fn handle(
    request: WorkerRequest,
    loaded: &mut Loaded,
    cancelled: &AtomicBool,
) -> (WorkerResponse, bool) {
    let meta = request.meta().clone();
    if meta.protocol_version != PROTOCOL_VERSION {
        return (
            error(
                meta,
                "protocol_version",
                "unsupported protocol version".into(),
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
        WorkerRequest::Load { meta, model_path } => match load(&model_path, loaded) {
            Ok(_) => WorkerResponse::ModelLoaded {
                protocol_version: PROTOCOL_VERSION,
                request_id: meta.request_id,
                backend: backend(),
            },
            Err(message) => error(meta, "model_load", message),
        },
        WorkerRequest::Transcribe {
            meta,
            model_path,
            language,
            samples_i16_base64,
        } => transcribe(
            meta,
            &model_path,
            &language,
            None,
            0,
            false,
            &samples_i16_base64,
            loaded,
            cancelled,
        ),
        WorkerRequest::TranscribeWindow {
            meta,
            model_path,
            language,
            context,
            audio_start_sample,
            samples_i16_base64,
        } => transcribe(
            meta,
            &model_path,
            &language,
            context.as_deref(),
            audio_start_sample,
            true,
            &samples_i16_base64,
            loaded,
            cancelled,
        ),
        WorkerRequest::CancelRequest { meta, .. } => error(
            meta,
            "request",
            "cancellation must use the control reader".into(),
        ),
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
    context: Option<&str>,
    audio_start_sample: u64,
    timed: bool,
    encoded: &str,
    loaded: &mut Loaded,
    cancelled: &AtomicBool,
) -> WorkerResponse {
    let samples = match decode_i16(encoded) {
        Ok(samples) => samples,
        Err(message) => return error(meta, "audio", message),
    };
    if timed && samples.len() > 30 * 16000 {
        return error(meta, "audio", "window exceeds 30 seconds".into());
    }
    if cancelled.load(Ordering::Acquire) {
        return error(meta, "cancelled", "request cancelled".into());
    }
    let inference = match load(model_path, loaded) {
        Ok(inference) => inference,
        Err(message) => return error(meta, "model_load", message),
    };
    let transcript = match inference.transcribe(
        &samples,
        language,
        context,
        audio_start_sample,
        backend(),
        timed,
        &|| cancelled.load(Ordering::Acquire),
    ) {
        Ok(transcript) => transcript,
        Err(message) => {
            return error(
                meta,
                if cancelled.load(Ordering::Acquire) {
                    "cancelled"
                } else {
                    "transcribe"
                },
                message,
            )
        }
    };
    if timed {
        WorkerResponse::WindowResult {
            protocol_version: PROTOCOL_VERSION,
            request_id: meta.request_id,
            operation_id: meta.operation_id.unwrap_or_default(),
            transcript,
        }
    } else {
        WorkerResponse::Result {
            protocol_version: PROTOCOL_VERSION,
            request_id: meta.request_id,
            operation_id: meta.operation_id.unwrap_or_default(),
            text: transcript.text,
            audio_secs: transcript.audio_secs,
            transcribe_secs: transcript.transcribe_secs,
            backend: backend(),
        }
    }
}

fn load<'a>(path: &str, loaded: &'a mut Loaded) -> Result<&'a mut InferenceState, String> {
    if loaded
        .as_ref()
        .map_or(true, |(loaded_path, _)| loaded_path != path)
    {
        if !Path::new(path).exists() {
            return Err(format!("model not found: {path}"));
        }
        let mut params = WhisperContextParameters::default();
        params.use_gpu(!matches!(backend(), BackendKind::Cpu));
        let context =
            WhisperContext::new_with_params(path, params).map_err(|error| error.to_string())?;
        *loaded = Some((path.into(), InferenceState::new(Arc::new(context))?));
    }
    Ok(&mut loaded.as_mut().ok_or("model state is unavailable")?.1)
}

fn decode_i16(value: &str) -> Result<Vec<i16>, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|error| error.to_string())?;
    if bytes.is_empty() {
        return Err("PCM payload is empty".into());
    }
    if bytes.len() % 2 != 0 {
        return Err("PCM payload has an odd byte length".into());
    }
    Ok(bytes
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
        .collect())
}

pub(crate) fn error(meta: RequestMeta, code: &str, message: String) -> WorkerResponse {
    WorkerResponse::Error {
        protocol_version: PROTOCOL_VERSION,
        request_id: meta.request_id,
        operation_id: meta.operation_id,
        code: code.into(),
        message,
    }
}

pub(crate) fn backend() -> BackendKind {
    if cfg!(feature = "cuda") {
        BackendKind::Cuda
    } else if cfg!(feature = "vulkan") {
        BackendKind::Vulkan
    } else {
        BackendKind::Cpu
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelled_window_never_loads_a_model() {
        let request = WorkerRequest::TranscribeWindow {
            meta: RequestMeta::new("window", Some("1".into())),
            model_path: "missing.bin".into(),
            language: "ru".into(),
            context: None,
            audio_start_sample: 16000,
            samples_i16_base64: "AAA=".into(),
        };
        let (response, _) = handle(request, &mut None, &AtomicBool::new(true));
        assert!(matches!(response, WorkerResponse::Error { code, .. } if code == "cancelled"));
    }
    #[test]
    fn odd_pcm_and_invalid_base64_are_rejected() {
        assert!(decode_i16("AQ==").is_err());
        assert!(decode_i16("!").is_err());
    }
}
